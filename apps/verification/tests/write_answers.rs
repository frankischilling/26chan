#![cfg(feature = "database-tests")]
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
use board_verification::{AppState, router};
use board_verification_client::protocol;
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;

async fn request<T: Serialize>(
    app: &Router,
    path: &str,
    value: &T,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {}", "11".repeat(32)))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(value).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn transported(value: board_domain::anonymous_session::Fingerprints) -> protocol::Fingerprints {
    protocol::Fingerprints {
        token: value.token,
        network: value.network,
        address: value.address,
        environment: value.environment,
    }
}

#[tokio::test]
async fn write_answer_routes_bind_scope_and_never_replay_receipts() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'wc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned write challenge','Synthetic',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned target','Synthetic challenge target')").bind(target).bind(&board).execute(&owner).await.unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let app = router(
        AppState::new(
            store.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let peer = "192.0.2.79".parse().unwrap();
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let work = {
        let owner = owner.clone();
        let store = store.clone();
        let board = board.clone();
        async move {
            store.ensure_session(&fingerprints, true).await.unwrap();
            for (purpose, stored, name, target) in [
                (
                    protocol::Purpose::Thread,
                    ChallengePurpose::Thread,
                    "thread",
                    0,
                ),
                (
                    protocol::Purpose::Reply,
                    ChallengePurpose::Reply,
                    "reply",
                    target,
                ),
                (
                    protocol::Purpose::Report,
                    ChallengePurpose::Report,
                    "report",
                    target,
                ),
            ] {
                for failure in [false, true] {
                    // Only the disposable fixture controls its peer cooldown.
                    sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
                    let context = protocol::ChallengeContext {
                        session: protocol::Session {
                            fingerprints: transported(fingerprints),
                            minted: false,
                            peer,
                            country: "US".into(),
                        },
                        purpose,
                        board: Some(board.clone()),
                        target,
                        form_nonce: "ab".repeat(32),
                        email_token: None,
                    };
                    let context_hash = key
                        .form_context(name, Some(&board), target, &context.form_nonce)
                        .unwrap();
                    let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
                    let answer = key.challenge_answer(&challenge, 6).unwrap();
                    let answer_hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
                    assert!(
                        store
                            .issue_challenge(&NewChallenge {
                                token_hash: &key.token_hash(&challenge),
                                answer_hash: &answer_hash,
                                peer_hash: &peer_hash,
                                session_hash: &fingerprints.token,
                                purpose: stored,
                                board: Some(&board),
                                target,
                                suspicious: false,
                                context_hash: &context_hash
                            })
                            .await
                            .unwrap()
                            .issued
                    );
                    let mut input = protocol::Answer {
                        context,
                        challenge: challenge.encode(),
                        answer: answer.as_str().into(),
                    };
                    let mut bad_context = input.context.clone();
                    bad_context.form_nonce = "cd".repeat(32);
                    let (status, value) = request(
                        &app,
                        "/v1/check",
                        &protocol::Answer {
                            context: bad_context,
                            challenge: input.challenge.clone(),
                            answer: input.answer.clone(),
                        },
                    )
                    .await;
                    assert_eq!(status, StatusCode::OK);
                    assert_eq!(value["state"], "rejected");
                    let mut email_context = input.context.clone();
                    email_context.email_token = Some("unexpected".into());
                    let (status, _) = request(
                        &app,
                        "/v1/check",
                        &protocol::Answer {
                            context: email_context,
                            challenge: input.challenge.clone(),
                            answer: input.answer.clone(),
                        },
                    )
                    .await;
                    assert_eq!(status, StatusCode::BAD_REQUEST);
                    if failure {
                        input.answer = "invalid".into();
                    }
                    let (status, value) = request(&app, "/v1/check", &input).await;
                    assert_eq!(status, StatusCode::OK);
                    if failure {
                        assert_eq!(value["state"], "rejected");
                    } else {
                        assert_eq!(value["state"], "verified");
                        let receipt =
                            ReceiptToken::parse(value["receipt"].as_str().unwrap()).unwrap();
                        let actual:(Vec<u8>,Vec<u8>,String,Option<String>,i64)=sqlx::query_as("SELECT peer_hash,context_hash,purpose,board,target FROM verification_private.receipts WHERE token_hash=$1").bind(receipt.storage_hash().as_slice()).fetch_one(&owner).await.unwrap();
                        assert_eq!(
                            actual,
                            (
                                peer_hash.to_vec(),
                                context_hash.to_vec(),
                                name.into(),
                                Some(board.clone()),
                                target
                            )
                        );
                        assert_eq!(value["peer_hash"], serde_json::json!(peer_hash));
                        assert_eq!(value["context_hash"], serde_json::json!(context_hash));
                    }
                    input.answer = answer.as_str().into();
                    let (_, replay) = request(&app, "/v1/check", &input).await;
                    assert_eq!(
                        replay["state"], "rejected",
                        "Success and wrong answers both consume the challenge once"
                    );
                }
            }
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    store.close().await;
    owner.close().await;
    outcome.unwrap();
}
