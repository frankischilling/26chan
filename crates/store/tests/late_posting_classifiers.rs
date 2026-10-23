#![cfg(feature = "database-tests")]
mod support;
use board_domain::{
    anonymous_session::{Capability, Fingerprints},
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    content_admission::{network::ProviderValue::Available as A, threat},
};
use board_store::{
    anonymous_session::PostingSession,
    posting_classifiers::RequestFacts,
    verification::{ChallengeCheck, ChallengePurpose, NewChallenge, VerificationStore},
    *,
};
use chrono::{Timelike, Utc};
use sqlx::PgPool;

// Use executed source cases for deterministic low/high scores. This fixture
// qualifies store transactions; the HTTP capture path has separate tests.
fn score(high: bool) -> threat::Score {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../domain/tests/fixtures/threat-score.json"
    ))
    .unwrap();
    let name = if high {
        "ua-0-type-1-mode-1"
    } else {
        "ua-7-type-5-mode-1"
    };
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap();
    let entries = |value: &serde_json::Value| -> Vec<(String, Vec<u8>)> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v[0].as_str().unwrap().to_owned(),
                    v[1].as_str().unwrap().as_bytes().to_vec(),
                )
            })
            .collect()
    };
    let server = entries(&case["server"]);
    let cookies = entries(&case["cookies"]);
    let server: Vec<_> = server
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let cookies: Vec<_> = cookies
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let A(score) = threat::score(A(threat::Request {
        server: &server,
        cookies: &cookies,
        challenge: None,
        response: None,
        request_time: 1000,
        country: None,
        is_op: false,
        multipart: true,
    })) else {
        panic!("Qualified score")
    };
    assert_eq!(score.value(), if high { 1.0 } else { 0.0 });
    score
}
async fn receipt(
    verifier: &VerificationStore,
    key: &VerificationKey,
    board: &str,
    fingerprints: &Fingerprints,
    peer: &[u8; 32],
    context: &[u8; 32],
) -> ReceiptToken {
    let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
    let answer = key.challenge_answer(&challenge, 6).unwrap();
    let hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
    let issued = verifier
        .issue_challenge(&NewChallenge {
            token_hash: &key.token_hash(&challenge),
            answer_hash: &hash,
            peer_hash: peer,
            session_hash: &fingerprints.token,
            purpose: ChallengePurpose::Thread,
            board: Some(board),
            target: 0,
            suspicious: false,
            context_hash: context,
        })
        .await
        .unwrap();
    assert!(issued.issued);
    let receipt = ReceiptToken::generate().unwrap();
    assert!(
        verifier
            .check_challenge(&ChallengeCheck {
                token_hash: &key.token_hash(&challenge),
                answer_hash: Some(&hash),
                peer_hash: peer,
                session_hash: &fingerprints.token,
                purpose: ChallengePurpose::Thread,
                board: Some(board),
                target: 0,
                context_hash: context,
                receipt_hash: &receipt.storage_hash()
            })
            .await
            .unwrap()
    );
    receipt
}
#[tokio::test]
async fn late_refusals_keep_only_source_state_and_do_not_consume_provisional_content() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'lp'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,robot9000,captcha_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned late posting','Synthetic',2000,100,100,100,10,true,true,true,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let peer = "192.0.2.111".parse().unwrap();
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let key = VerificationKey::parse(&ReceiptToken::generate().unwrap().encode()[3..]).unwrap();
    let peer_hash = key.network_identity(peer);
    let context_hash = key
        .form_context("thread", Some(&board), 0, &"ab".repeat(32))
        .unwrap();
    let (o, p, v, b) = (
        owner.clone(),
        public.clone(),
        verifier.clone(),
        board.clone(),
    );
    let result=tokio::spawn(async move {
        v.ensure_session(&fingerprints,true).await.unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-100,network_at=extract(epoch FROM statement_timestamp())::bigint-100,environment_at=extract(epoch FROM statement_timestamp())::bigint-100,activity_at=extract(epoch FROM statement_timestamp())::bigint-60 WHERE token_hash=$1").bind(fingerprints.token.as_slice()).execute(&o).await.unwrap();
        let first=receipt(&v,&key,&b,&fingerprints,&peer_hash,&context_hash).await;
        let mut current=first;
        let poster_key=support::fresh_key();
        let comments=["Owned missing evidence", "Owned hostile signature", "Owned clean submission", "Owned changed environment", "Owned retained country history", "Owned foreign request scope", "Owned disabled policy"];
        let expected=[(0,0,0),(0,1,1),(1,2,1),(1,3,3),(1,4,4),(1,4,4),(2,5,4)];
        let mut country_event=None;
        for (attempt,comment) in comments.into_iter().enumerate() {
            if attempt==3 {
                // This test exercises posting transactions, not the separate
                // challenge-refresh timer. Expire only this owned peer's wait.
                sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&o).await.unwrap();
                current=receipt(&v,&key,&b,&fingerprints,&peer_hash,&context_hash).await;
            }
            if attempt==6 {sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1").bind(&b).execute(&o).await.unwrap();}
            let now=Utc::now().with_nanosecond(0).unwrap();
            let posting=PostingContext {request_start:now,peer:Some(peer),op_password_proof:None};
            let changed=Fingerprints {environment:if attempt>=3 {[9;32]} else {fingerprints.environment},..fingerprints};
            let facts=if attempt==0 || attempt==6 {None} else {Some(RequestFacts::new(posting,if attempt==5 {1} else {0},&b,A(score(attempt==1)),A(*b"US"),A(None)))};
            let outcome=create_post_with_challenge(&p,&b,0,&NewPost {name:"Anonymous".into(),subject:"Owned subject".into(),comment:comment.into(),deletion_hash:"owned-fixture".into(),sage:false},None,
                ProtectedPostingContext {request_facts:facts,anonymous:AnonymousPostingContext {posting,session:PostingSession {fingerprints:changed,minted:false,now}},challenge:if attempt==6 {None} else {WriteChallengeProof::from_issuer(&current.encode(),peer_hash,context_hash)}},
                PostMetadata {
            event_scores: false,drawing:None,spoiler:false,keys:PostIdentityKeys {tripcode:None,poster_id:Some(&poster_key)},country_database:None,flag:"",options:""}).await;
            match attempt {
                0|5=>assert!(matches!(outcome,Err(StoreError::Database(_))),"{outcome:?}"),
                1=>match outcome {Err(StoreError::PostingClassifierRejected(refusal))=>{assert!(refusal.quiet_post().is_some());assert!(!refusal.cookie_refreshed());},other=>panic!("Expected quiet refusal: {other:?}")},
                3|4=>match outcome {Err(StoreError::PostingClassifierRejected(refusal))=>{assert!(refusal.quiet_post().is_none());assert!(refusal.cookie_refreshed());},other=>panic!("Expected country refusal: {other:?}")},
                _=>{outcome.unwrap();},
            }
            let saved:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.classifier_events WHERE board=$1)").bind(&b).fetch_one(&o).await.unwrap();
            assert_eq!(saved,expected[attempt],"attempt {attempt}");
            let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)").bind(current.storage_hash().as_slice()).fetch_one(&o).await.unwrap();
            assert_eq!(retained,attempt!=2,"attempt {attempt}");
            if attempt==3 || attempt==4 {
                let event:i64=sqlx::query_scalar("SELECT created_at FROM post_secrets.classifier_history WHERE token_hash=$1 AND kind='country_changed'").bind(fingerprints.token.as_slice()).fetch_one(&o).await.unwrap();
                if let Some(previous)=country_event {assert_eq!(previous,event);} country_event=Some(event);
                let count:i32=sqlx::query_scalar("SELECT posts::integer+CASE WHEN (pending&1)<>0 THEN 1 ELSE 0 END FROM post_secrets.anonymous_sessions WHERE token_hash=$1").bind(fingerprints.token.as_slice()).fetch_one(&o).await.unwrap();
                assert_eq!(count,1);
            }
        }
    }).await;
    let mut tx = support::begin_cleanup_with_sessions(
        &owner,
        std::slice::from_ref(&board),
        &[fingerprints.token],
    )
    .await;
    for query in [
        "DELETE FROM post_secrets.classifier_events WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peer_risk WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}

// Session issuance and request-start clocks use elapsed whole seconds. A
// PostgreSQL numeric-to-bigint cast rounds, which can put a new session ahead
// of an immediately following request during the second half of a second.
#[tokio::test]
async fn verification_session_timestamps_do_not_round_into_the_future() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier = PgPool::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut failures = Vec::new();
    for _ in 0..3 {
        let fingerprints = Capability::generate().unwrap().fingerprints(None, *b"US");
        let (issued, before, after): (bool, i64, i64) = sqlx::query_as(
            "WITH waited AS MATERIALIZED (SELECT pg_sleep(mod(1.65-mod(extract(epoch FROM clock_timestamp()),1),1)::double precision)), started AS MATERIALIZED (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint AS before FROM waited), issued AS MATERIALIZED (SELECT before,content.verification_ensure_session($1,$2,$3,$4,true) AS issued FROM started) SELECT issued,before,floor(extract(epoch FROM clock_timestamp()))::bigint AS after FROM issued"
        ).bind(fingerprints.token.as_slice()).bind(fingerprints.network.as_slice())
         .bind(fingerprints.address.as_slice()).bind(fingerprints.environment.as_slice())
         .fetch_one(&verifier).await.unwrap();
        let times: (i64,i64,i64,i64) = sqlx::query_as("SELECT created_at,network_at,address_at,environment_at FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(fingerprints.token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
        assert!(issued);
        for at in [times.0, times.1, times.2, times.3] {
            if !(before..=after).contains(&at) {
                failures.push((before, at, after));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Session clock escaped issuance interval: {failures:?}"
    );
}
