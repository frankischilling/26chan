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
use board_store::verification::VerificationStore;
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
async fn posting_issuer_preserves_ticket_wait_and_separates_classic_and_disabled_boards() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'pi'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Posting issuer','Synthetic',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
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
    let peer = "192.0.2.83".parse().unwrap();
    let fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let work = {
        let owner = owner.clone();
        let board = board.clone();
        async move {
            let mut input = protocol::PostingChallengeRequest {
                context: protocol::ChallengeContext {
                    session: protocol::Session {
                        fingerprints: transported(fp),
                        minted: true,
                        peer,
                        country: "US".into(),
                    },
                    purpose: protocol::Purpose::Thread,
                    board: Some(board.clone()),
                    target: 0,
                    form_nonce: "ab".repeat(32),
                    email_token: None,
                },
                ticket: None,
                ticket_answer: None,
            };
            let (status, initial) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(initial["ticket_challenge"], true);
            assert_eq!(initial["challenge"]["state"], "ready");
            let challenge = VerificationToken::parse(
                TokenKind::Challenge,
                initial["challenge"]["token"].as_str().unwrap(),
            )
            .unwrap();
            let answer = key.challenge_answer(&challenge, 8).unwrap();
            let (_, not_post) = request(
                &app,
                "/v1/check",
                &protocol::Answer {
                    context: input.context.clone(),
                    challenge: challenge.encode(),
                    answer: answer.as_str().into(),
                },
            )
            .await;
            assert_eq!(
                not_post["state"], "rejected",
                "Ticket escalation cannot authorize a post"
            );
            input.context.session.minted = false;
            input.ticket_answer = Some(protocol::TicketAnswer {
                challenge: challenge.encode(),
                answer: answer.as_str().into(),
            });
            let (status, wait) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(wait["ticket_challenge"], false);
            assert_eq!(wait["challenge"]["state"], "wait");
            assert_eq!(wait["challenge"]["seconds"], 900);
            let ticket = wait["challenge"]["ticket"].as_str().unwrap().to_owned();
            let token = VerificationToken::parse(TokenKind::Ticket, &ticket).unwrap();
            let ticket_hash = key.token_hash(&token);
            input.ticket = Some(ticket.clone());
            input.ticket_answer = None;
            let (_, resumed) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(resumed["challenge"]["ticket"], ticket);
            assert!(resumed["challenge"]["seconds"].as_u64().unwrap() <= 900);
            sqlx::query("UPDATE verification_private.tickets SET created_at=statement_timestamp()-interval '901 seconds',expires_at=statement_timestamp()+interval '2699 seconds' WHERE token_hash=$1").bind(ticket_hash.as_slice()).execute(&owner).await.unwrap();
            reset_peer(&owner, &peer_hash).await;
            let (status, ordinary) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(ordinary["ticket_challenge"], false);
            assert_eq!(ordinary["challenge"]["state"], "ready");
            let challenge = VerificationToken::parse(
                TokenKind::Challenge,
                ordinary["challenge"]["token"].as_str().unwrap(),
            )
            .unwrap();
            let answer = key.challenge_answer(&challenge, 6).unwrap();
            let (_, solved) = request(
                &app,
                "/v1/check",
                &protocol::Answer {
                    context: input.context.clone(),
                    challenge: challenge.encode(),
                    answer: answer.as_str().into(),
                },
            )
            .await;
            assert_eq!(solved["state"], "verified");
            sqlx::query("UPDATE content.boards SET captcha_twister=false WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            input.ticket = None;
            reset_peer(&owner, &peer_hash).await;
            let (_, classic) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(classic["ticket_challenge"], false);
            assert_eq!(classic["challenge"]["state"], "ready");
            assert_eq!(classic["challenge"]["slider"], false);
            sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            let (_, disabled) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(disabled["challenge"]["state"], "disabled");
            sqlx::query(
                "UPDATE content.boards SET captcha_enabled=true,captcha_twister=true WHERE slug=$1",
            )
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
            sqlx::query("UPDATE verification_private.tickets SET created_at=statement_timestamp()-interval '3601 seconds',expires_at=statement_timestamp()-interval '1 second' WHERE token_hash=$1").bind(ticket_hash.as_slice()).execute(&owner).await.unwrap();
            input.ticket = Some(ticket);
            reset_peer(&owner, &peer_hash).await;
            let (_, expired) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(expired["ticket_challenge"], true);
            sqlx::query("INSERT INTO verification_private.peer_risk(peer_hash,score,expires_at) VALUES($1,99,statement_timestamp()+interval '1 hour')").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
            let (_, trusted) = request(&app, "/v1/posting/challenge", &input).await;
            assert_eq!(trusted["ticket_challenge"], false);
            assert_eq!(trusted["challenge"]["state"], "wait");
            assert_eq!(trusted["challenge"]["seconds"], 900);
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query("DELETE FROM verification_private.tickets WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fp.token.as_slice())
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
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    store.close().await;
    owner.close().await;
    outcome.unwrap();
}
async fn reset_peer(owner: &PgPool, peer: &[u8; 32]) {
    sqlx::query("UPDATE verification_private.peers SET refresh_after=statement_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer.as_slice()).execute(owner).await.unwrap();
}
