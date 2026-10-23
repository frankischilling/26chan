#![cfg(feature = "database-tests")]
#[path = "site_submissions/applications.rs"]
mod applications;
#[path = "site_submissions/feedback.rs"]
mod feedback;
#[path = "site_submissions/pass_vote.rs"]
mod pass_vote;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    verification_mail::MailKey,
};
use board_store::verification::{ChallengePurpose, NewChallenge, VerificationStore};
use board_verification_client::protocol;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;

async fn call(
    app: &Router,
    path: &str,
    input: &impl serde::Serialize,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {}", "11".repeat(32)))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(input).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 262144).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
async fn consume(
    owner: &PgPool,
    receipt: &[u8; 32],
    peer: &[u8; 32],
    session: &[u8; 32],
    context: &[u8; 32],
    scope: (&str, i64),
    commit: bool,
) -> Result<(), sqlx::Error> {
    let mut tx = owner.begin().await?;
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("SELECT submission_api.consume_captcha($1,$2,$3,$4,$5,$6)")
        .bind(receipt.as_slice())
        .bind(peer.as_slice())
        .bind(session.as_slice())
        .bind(scope.0)
        .bind(scope.1)
        .bind(context.as_slice())
        .execute(&mut *tx)
        .await;
    match result {
        Ok(_) if commit => tx.commit().await,
        Ok(_) => tx.rollback().await,
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
#[tokio::test]
async fn site_captcha_receipts_are_closed_bound_atomic_and_separate_from_posting() {
    let owner = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let app = board_verification::router(
        board_verification::AppState::new(
            store.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let peer = "192.0.2.225".parse().unwrap();
    let fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let context = protocol::ChallengeContext {
        session: protocol::Session {
            fingerprints: protocol::Fingerprints {
                token: fp.token,
                network: fp.network,
                address: fp.address,
                environment: fp.environment,
            },
            minted: false,
            peer,
            country: "US".into(),
        },
        purpose: protocol::Purpose::SiteFeedback,
        board: None,
        target: id,
        form_nonce: "ab".repeat(32),
        email_token: None,
    };
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let store=store.clone();async move {
        store.ensure_session(&fp,true).await.unwrap();
        let request=protocol::PostingChallengeRequest {context:context.clone(),ticket:None,ticket_answer:None};
        assert_eq!(call(&app,"/v1/posting/challenge",&request).await.0,StatusCode::NOT_FOUND);
        sqlx::query("INSERT INTO submission_private.feedback_profiles(id,enabled,subject_limit,message_limit,capacity) VALUES($1,true,150,2000,100)").bind(id).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.feedback_categories VALUES($1,1,0,'Owned','Owned')").bind(id).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.application_campaigns(id,enabled,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity) VALUES($1,true,1,253402300799,ARRAY['g'],ARRAY['One','Two','Three','Four'],150,2000,100)").bind(id).execute(&owner).await.unwrap();
        let application:i64=sqlx::query_scalar("INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at) VALUES($1,25,0,1,'Owned','g',ARRAY['a','b','c','d'],1,1) RETURNING id").bind(id).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.contest_campaigns(id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity) VALUES($1,1,true,ARRAY['g'],1,253402300797,253402300798,253402300799,100,100)").bind(id).execute(&owner).await.unwrap();
        let entry:i64=sqlx::query_scalar("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,media_id,created_at,updated_at) VALUES($1,'g',decode(repeat('01',32),'hex'),decode(repeat('02',32),'hex'),'Owned',repeat('a',32),1,1) RETURNING id").bind(id).fetch_one(&owner).await.unwrap();
        let (status,rendered)=call(&app,"/v1/posting/challenge",&request).await;
        assert_eq!(status,StatusCode::OK);
        assert_eq!(rendered["ticket_challenge"],false);
        assert_eq!(rendered["challenge"]["state"],"ready");
        for (purpose,stored,name,target) in [
            (protocol::Purpose::SiteFeedback,ChallengePurpose::SiteFeedback,"site_feedback",id),
            (protocol::Purpose::SiteApplication,ChallengePurpose::SiteApplication,"site_application",id),
            (protocol::Purpose::SiteApplicationAuth,ChallengePurpose::SiteApplicationAuth,"site_application_auth",application),
            (protocol::Purpose::SiteApplicationEdit,ChallengePurpose::SiteApplicationEdit,"site_application_edit",application),
            (protocol::Purpose::SiteContestSubmit,ChallengePurpose::SiteContestSubmit,"site_contest_submit",id),
            (protocol::Purpose::SiteContestDelete,ChallengePurpose::SiteContestDelete,"site_contest_delete",entry),
        ] {
            let mut context=context.clone();context.purpose=purpose;context.target=target;
            let hash=key.form_context(name,None,target,&context.form_nonce).unwrap();
            let token=VerificationToken::generate(TokenKind::Challenge).unwrap();
            sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
            let issued=store.issue_challenge(&NewChallenge {token_hash:&key.token_hash(&token),answer_hash:&key.answer_hash(&token,"ADGJ").unwrap(),peer_hash:&peer_hash,session_hash:&fp.token,purpose:stored,board:None,target,suspicious:false,context_hash:&hash}).await.unwrap();
            assert!(issued.issued);
            let mut input=protocol::Answer {context,challenge:token.encode(),answer:"ADGJ".into()};
            input.context.form_nonce="cd".repeat(32);
            let (status,rejected)=call(&app,"/v1/check",&input).await;
            assert_eq!(status,StatusCode::OK);assert_eq!(rejected["state"],"rejected");
            input.context.form_nonce="ab".repeat(32);
            let (status,accepted)=call(&app,"/v1/check",&input).await;
            assert_eq!(status,StatusCode::OK);assert_eq!(accepted["state"],"verified");
            let receipt=ReceiptToken::parse(accepted["receipt"].as_str().unwrap()).unwrap();
            let digest=receipt.storage_hash();
            let denied=sqlx::query("SELECT submission_api.consume_captcha($1,$2,$3,$4,$5,$6)").bind(digest.as_slice()).bind(peer_hash.as_slice()).bind(fp.token.as_slice()).bind(name).bind(target).bind(hash.as_slice()).execute(&public).await.unwrap_err();
            assert_eq!(denied.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *tx).await.unwrap();
            let refused = sqlx::query("SELECT verification_private.take_write_receipt_data($1,$2,$3,$4,NULL,$5,$6)")
                .bind(digest.as_slice()).bind(peer_hash.as_slice()).bind(fp.token.as_slice())
                .bind(name).bind(target).bind(hash.as_slice()).execute(&mut *tx).await.unwrap_err();
            assert_eq!(refused.as_database_error().and_then(|e|e.code()).as_deref(),Some("23514"));
            tx.rollback().await.unwrap();
            if name == "site_feedback" {
                assert!(consume(&owner,&digest,&peer_hash,&fp.token,&hash,("site_application",id),true).await.is_err());
                sqlx::query("UPDATE submission_private.feedback_profiles SET enabled=false WHERE id=$1")
                    .bind(id).execute(&owner).await.unwrap();
                let closed=consume(&owner,&digest,&peer_hash,&fp.token,&hash,(name,target),true).await.unwrap_err();
                assert_eq!(closed.as_database_error().and_then(|e|e.code()).as_deref(),Some("P0002"));
                sqlx::query("UPDATE submission_private.feedback_profiles SET enabled=true WHERE id=$1")
                    .bind(id).execute(&owner).await.unwrap();
            }
            let mut expired = owner.begin().await.unwrap();
            sqlx::query("UPDATE verification_private.receipts SET created_at=clock_timestamp()-interval '60 seconds',expires_at=clock_timestamp()-interval '40 seconds' WHERE token_hash=$1")
                .bind(digest.as_slice()).execute(&mut *expired).await.unwrap();
            sqlx::query("SET LOCAL ROLE board_submission_owner").execute(&mut *expired).await.unwrap();
            let refused=sqlx::query("SELECT submission_api.consume_captcha($1,$2,$3,$4,$5,$6)")
                .bind(digest.as_slice()).bind(peer_hash.as_slice()).bind(fp.token.as_slice())
                .bind(name).bind(target).bind(hash.as_slice()).execute(&mut *expired).await.unwrap_err();
            assert_eq!(refused.as_database_error().and_then(|e|e.code()).as_deref(),Some("P0166"));
            expired.rollback().await.unwrap();
            let wrong=consume(&owner,&digest,&peer_hash,&fp.token,&[0;32],(name,target),true).await.unwrap_err();
            assert_eq!(wrong.as_database_error().and_then(|e|e.code()).as_deref(),Some("P0166"));
            consume(&owner,&digest,&peer_hash,&fp.token,&hash,(name,target),false).await.unwrap();
            consume(&owner,&digest,&peer_hash,&fp.token,&hash,(name,target),true).await.unwrap();
            assert!(consume(&owner,&digest,&peer_hash,&fp.token,&hash,(name,target),true).await.is_err());
        }
        sqlx::query("UPDATE submission_private.application_campaigns SET ends_at=floor(extract(epoch FROM clock_timestamp()))::bigint+2 WHERE id=$1")
            .bind(id).execute(&owner).await.unwrap();
        let mut clock=owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_submission_owner").execute(&mut *clock).await.unwrap();
        sqlx::query("SELECT set_config('owned.site_campaign',$1,true)").bind(id.to_string()).execute(&mut *clock).await.unwrap();
        sqlx::query("DO $$ BEGIN IF submission_api.challenge_open('site_application',current_setting('owned.site_campaign')::bigint) IS DISTINCT FROM true THEN RAISE EXCEPTION 'Owned application window was not open'; END IF; PERFORM pg_sleep(2.2); IF submission_api.challenge_open('site_application',current_setting('owned.site_campaign')::bigint) IS DISTINCT FROM false THEN RAISE EXCEPTION 'Application gate used stale statement time'; END IF; END $$;")
            .execute(&mut *clock).await.unwrap();
        clock.rollback().await.unwrap();
        sqlx::query("UPDATE submission_private.application_campaigns SET ends_at=253402300799 WHERE id=$1")
            .bind(id).execute(&owner).await.unwrap();
        feedback::exercise(&owner, &public, &app, &store, &key, &context).await;
        applications::exercise(&owner, &public, &app, &store, &key, &context).await;
        let mut vote=context.clone();vote.purpose=protocol::Purpose::SiteContestVote;
        let request=protocol::PostingChallengeRequest {context:vote,ticket:None,ticket_answer:None};
        assert_eq!(call(&app,"/v1/posting/challenge",&request).await.0,StatusCode::BAD_REQUEST);
        pass_vote::exercise(&owner, &public, &app, &key, context, id, entry).await;
    }}).await;
    for query in [
        "DELETE FROM submission_private.feedback WHERE profile_id=$1",
        "DELETE FROM submission_private.contest_entries WHERE campaign_id=$1",
        "DELETE FROM submission_private.applications WHERE campaign_id=$1",
        "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
        "DELETE FROM submission_private.application_campaigns WHERE id=$1",
        "DELETE FROM submission_private.contest_campaigns WHERE id=$1",
    ] {
        sqlx::query(query).bind(id).execute(&owner).await.unwrap();
    }
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fp.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    store.close().await;
    public.close().await;
    owner.close().await;
    result.unwrap();
}
