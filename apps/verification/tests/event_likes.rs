#![cfg(feature = "database-tests")]
#[path = "event_likes/ledger.rs"]
mod ledger;
#[path = "event_likes/pass.rs"]
mod pass;
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

async fn answer(app: &Router, input: &protocol::Answer) -> serde_json::Value {
    call(app, "/v1/check", input).await
}
async fn call(app: &Router, path: &str, input: &impl serde::Serialize) -> serde_json::Value {
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
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), 262144).await.unwrap()).unwrap()
}
async fn consume(
    owner: &PgPool,
    receipt: &[u8; 32],
    peer: &[u8; 32],
    session: &[u8; 32],
    context: &[u8; 32],
    scope: (&str, i64, &str),
) -> Result<bool, sqlx::Error> {
    let (board, target, purpose) = scope;
    let mut tx = owner.begin().await?;
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await?;
    let result=sqlx::query_scalar("SELECT (verification_private.take_write_receipt_data($1,$2,$3,$4,$5,$6,$7)).purpose='like'")
        .bind(receipt.as_slice()).bind(peer.as_slice()).bind(session.as_slice()).bind(purpose).bind(board).bind(target).bind(context.as_slice()).fetch_one(&mut *tx).await;
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
#[tokio::test]
async fn event_like_proofs_stay_closed_and_cannot_authorize_posts_or_reports() {
    let owner = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM event_private.runs WHERE enabled")
            .fetch_one(&owner)
            .await
            .unwrap(),
        0,
        "Use the owned disposable database without operator events"
    );
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'el'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned like proof','Synthetic',1000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned proof target','Synthetic')").bind(target).bind(&board).execute(&owner).await.unwrap();
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
    let peer = "192.0.2.83".parse().unwrap();
    let fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let run = target;
    sqlx::query("INSERT INTO event_private.runs(id,kind,enabled,starts_at,ends_at) VALUES($1,3,false,floor(extract(epoch FROM now()))::bigint-60,floor(extract(epoch FROM now()))::bigint+3600)").bind(run).execute(&owner).await.unwrap();
    let work = {
        let owner = owner.clone();
        let store = store.clone();
        let board = board.clone();
        async move {
            store.ensure_session(&fp, true).await.unwrap();
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
                purpose: protocol::Purpose::Like,
                board: Some(board.clone()),
                target,
                form_nonce: board_domain::source_events::scores::form_nonce(run, &"ab".repeat(32))
                    .unwrap(),
                email_token: None,
            };
            let context_hash = key
                .form_context("like", Some(&board), target, &context.form_nonce)
                .unwrap();
            let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
            let solution = key.challenge_answer(&challenge, 6).unwrap();
            let answer_hash = key.answer_hash(&challenge, solution.as_str()).unwrap();
            let token_hash = key.token_hash(&challenge);
            let input = NewChallenge {
                token_hash: &token_hash,
                answer_hash: &answer_hash,
                peer_hash: &peer_hash,
                session_hash: &fp.token,
                purpose: ChallengePurpose::Like,
                board: Some(&board),
                target,
                context_hash: &context_hash,
                suspicious: false,
            };
            assert!(store.issue_challenge(&input).await.is_err());
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM verification_private.challenges WHERE token_hash=$1"
                )
                .bind(token_hash.as_slice())
                .fetch_one(&owner)
                .await
                .unwrap(),
                0
            );
            sqlx::query("UPDATE event_private.runs SET enabled=true WHERE id=$1")
                .bind(run)
                .execute(&owner)
                .await
                .unwrap();
            assert!(store.issue_challenge(&input).await.unwrap().issued);
            let input = protocol::Answer {
                context,
                challenge: challenge.encode(),
                answer: solution.as_str().into(),
            };
            for wrong in [protocol::Purpose::Report, protocol::Purpose::Reply] {
                let mut changed = input.context.clone();
                changed.purpose = wrong;
                let result = answer(
                    &app,
                    &protocol::Answer {
                        context: changed,
                        challenge: input.challenge.clone(),
                        answer: input.answer.clone(),
                    },
                )
                .await;
                assert_eq!(result["state"], "rejected");
            }
            let result = answer(&app, &input).await;
            assert_eq!(result["state"], "verified");
            let receipt = ReceiptToken::parse(result["receipt"].as_str().unwrap())
                .unwrap()
                .storage_hash();
            let flags:(bool,bool)=sqlx::query_as("SELECT from_credit,may_grant_credit FROM verification_private.receipts WHERE token_hash=$1").bind(receipt.as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(flags, (false, false));
            for purpose in ["report", "reply"] {
                assert!(
                    consume(
                        &owner,
                        &receipt,
                        &peer_hash,
                        &fp.token,
                        &context_hash,
                        (&board, target, purpose)
                    )
                    .await
                    .is_err()
                );
            }
            let reset_context = key
                .form_context(
                    "like",
                    Some(&board),
                    target,
                    &board_domain::source_events::scores::form_nonce(run + 1, &"ab".repeat(32))
                        .unwrap(),
                )
                .unwrap();
            assert!(
                consume(
                    &owner,
                    &receipt,
                    &peer_hash,
                    &fp.token,
                    &reset_context,
                    (&board, target, "like")
                )
                .await
                .is_err()
            );
            assert!(
                consume(
                    &owner,
                    &receipt,
                    &peer_hash,
                    &fp.token,
                    &context_hash,
                    (&board, target, "like")
                )
                .await
                .unwrap()
            );
            assert!(
                consume(
                    &owner,
                    &receipt,
                    &peer_hash,
                    &fp.token,
                    &context_hash,
                    (&board, target, "like")
                )
                .await
                .is_err()
            );
            assert_eq!(answer(&app, &input).await["state"], "rejected");
            // The inverse direction also refuses a posting/reporting proof
            // at the like consumer, even with its correct nonce and identity.
            for (purpose, stored, name) in [
                (protocol::Purpose::Reply, ChallengePurpose::Reply, "reply"),
                (
                    protocol::Purpose::Report,
                    ChallengePurpose::Report,
                    "report",
                ),
            ] {
                sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
                let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
                let solution = key.challenge_answer(&token, 6).unwrap();
                let mut context = input.context.clone();
                context.purpose = purpose;
                let hash = key
                    .form_context(name, Some(&board), target, &context.form_nonce)
                    .unwrap();
                assert!(
                    store
                        .issue_challenge(&NewChallenge {
                            token_hash: &key.token_hash(&token),
                            answer_hash: &key.answer_hash(&token, solution.as_str()).unwrap(),
                            peer_hash: &peer_hash,
                            session_hash: &fp.token,
                            purpose: stored,
                            board: Some(&board),
                            target,
                            context_hash: &hash,
                            suspicious: false
                        })
                        .await
                        .unwrap()
                        .issued
                );
                let proof = answer(
                    &app,
                    &protocol::Answer {
                        context,
                        challenge: token.encode(),
                        answer: solution.as_str().into(),
                    },
                )
                .await;
                assert_eq!(proof["state"], "verified");
                let receipt = ReceiptToken::parse(proof["receipt"].as_str().unwrap())
                    .unwrap()
                    .storage_hash();
                assert!(
                    consume(
                        &owner,
                        &receipt,
                        &peer_hash,
                        &fp.token,
                        &hash,
                        (&board, target, "like")
                    )
                    .await
                    .is_err()
                );
                assert!(
                    !consume(
                        &owner,
                        &receipt,
                        &peer_hash,
                        &fp.token,
                        &hash,
                        (&board, target, name)
                    )
                    .await
                    .unwrap()
                );
            }
            ledger::exercise(&owner, &store, &key, input.context.clone(), run).await;
            // Like images remain required on a CAPTCHA-disabled posting board.
            sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
            let challenge_request = protocol::PostingChallengeRequest {
                context: input.context.clone(),
                ticket: None,
                ticket_answer: None,
            };
            let issued = call(&app, "/v1/posting/challenge", &challenge_request).await;
            assert_eq!(issued["ticket_challenge"], false);
            assert_eq!(issued["challenge"]["state"], "ready");
            assert_eq!(issued["challenge"]["slider"], false);
            assert!(
                issued["challenge"]["image"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:image/png;base64,iVBORw0KGgo")
            );
            sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            // An already issued challenge also loses authority after closure.
            sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
            let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
            let solution = key.challenge_answer(&token, 6).unwrap();
            assert!(
                store
                    .issue_challenge(&NewChallenge {
                        token_hash: &key.token_hash(&token),
                        answer_hash: &key.answer_hash(&token, solution.as_str()).unwrap(),
                        peer_hash: &peer_hash,
                        session_hash: &fp.token,
                        purpose: ChallengePurpose::Like,
                        board: Some(&board),
                        target,
                        context_hash: &context_hash,
                        suspicious: false
                    })
                    .await
                    .unwrap()
                    .issued
            );
            sqlx::query("UPDATE event_private.runs SET enabled=false WHERE id=$1")
                .bind(run)
                .execute(&owner)
                .await
                .unwrap();
            assert_eq!(
                answer(
                    &app,
                    &protocol::Answer {
                        context: input.context.clone(),
                        challenge: token.encode(),
                        answer: solution.as_str().into()
                    }
                )
                .await["state"],
                "rejected"
            );
            let closed = app
                .clone()
                .oneshot(
                    Request::post("/v1/posting/challenge")
                        .header("authorization", format!("Bearer {}", "11".repeat(32)))
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&challenge_request).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(closed.status(), StatusCode::NOT_FOUND);
            pass::exercise(&owner, &app, &key, input.context).await;
        }
    };
    let result = tokio::spawn(work).await;
    sqlx::query("DELETE FROM event_private.runs WHERE id=$1")
        .bind(run)
        .execute(&owner)
        .await
        .unwrap();
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
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("SELECT pg_advisory_unlock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    store.close().await;
    owner.close().await;
    result.unwrap();
}
