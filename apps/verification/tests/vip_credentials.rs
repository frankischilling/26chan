#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_domain::{
    challenge_secrets::{ReceiptToken, VerificationKey},
    verification_mail::MailKey,
};
use board_store::verification::{VerificationStore, vip::LoginRequest};
use board_verification::{AppState, router};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};
use tower::ServiceExt;

fn provision(database: &str, key: &str, operation: Value) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-vip-admin"));
    command
        .env_clear()
        .env("MIGRATION_DATABASE_URL", database)
        .env("VERIFICATION_KEY", key)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(value) = std::env::var_os("LD_LIBRARY_PATH") {
        command.env("LD_LIBRARY_PATH", value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(operation.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
fn provision_ok(key: &str, operation: Value) -> i64 {
    let output = provision(
        &std::env::var("MIGRATION_DATABASE_URL").unwrap(),
        key,
        operation,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1);
    assert!(output.stderr.is_empty());
    value["id"].as_i64().unwrap()
}

struct Fixture {
    owner: PgPool,
    store: VerificationStore,
    material: String,
    identifier: [u8; 32],
    board: String,
    peers: Vec<[u8; 32]>,
}
impl Fixture {
    async fn new() -> Self {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let store =
            VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
                .await
                .unwrap();
        let material = ReceiptToken::generate().unwrap().encode()[3..].to_owned();
        let key = VerificationKey::parse(&material).unwrap();
        let identifier = key.vip_identifier("owner").unwrap();
        let board = format!("v{}", &material[..9]);
        let peers = ["192.0.2.31", "198.51.100.31", "203.0.113.31"]
            .map(|ip| key.network_identity(ip.parse().unwrap()))
            .to_vec();
        sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned VIP','Synthetic',false,2000,100,100,100,10)")
            .bind(&board).execute(&owner).await.unwrap();
        assert!(
            provision_ok(
                &material,
                json!({"action":"create","identifier":"owner",
            "name":"Owned Verified Name","password":"owned-vip-password"})
            ) > 0
        );
        Self {
            owner,
            store,
            material,
            identifier,
            board,
            peers,
        }
    }
    fn app(&self) -> Router {
        router(
            AppState::new(
                self.store.clone(),
                VerificationKey::parse(&self.material).unwrap(),
                MailKey::parse(&self.material).unwrap(),
                &"11".repeat(32),
            )
            .unwrap(),
        )
    }
    fn request(&self, password: &str, peer: &str) -> Value {
        json!({
            "session": {
                "fingerprints": {
                    "token": self.identifier, "network": self.identifier,
                    "address": self.identifier, "environment": self.identifier
                },
                "minted": true, "peer": peer, "country": "US"
            },
            "board": self.board, "target": 0,
            "options": format!("capcode_!owner!{password}"),
            "pass_proof": null
        })
    }
    async fn cleanup(self) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        for peer in &self.peers {
            sqlx::query("DELETE FROM verification_private.vip_attempts WHERE peer=$1")
                .bind(peer.as_slice())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM verification_private.vip_accounts WHERE identifier_hash=$1")
            .bind(self.identifier.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(&self.board)
            .execute(&self.owner)
            .await
            .unwrap();
        self.owner.close().await;
    }
}

async fn call(app: &Router, path: &str, value: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {}", "11".repeat(32)))
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn private_vip_http_qualifies_passwords_bypass_flood_and_revocation() {
    let fixture = Arc::new(Fixture::new().await);
    let work = {
        let fixture = fixture.clone();
        async move {
            let app = fixture.app();
            assert_eq!(
                call(&app, "/v1/vip/ready", json!({})).await,
                (StatusCode::OK, json!({"ready":true}))
            );
            let response = app
                .clone()
                .oneshot(
                    Request::post("/v1/vip/receipt")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            fixture
                                .request("owned-vip-password", "192.0.2.31")
                                .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            let (status, accepted) = call(
                &app,
                "/v1/vip/receipt",
                fixture.request("owned-vip-password", "192.0.2.31"),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(accepted["state"], "ready");
            let receipt = ReceiptToken::parse(accepted["receipt"].as_str().unwrap()).unwrap();
            assert_eq!(accepted.as_object().unwrap().len(), 4);
            let serialized = accepted.to_string();
            for private in [
                "owned-vip-password",
                "Owned Verified Name",
                "argon2",
                "owner",
            ] {
                assert!(!serialized.contains(private));
            }
            let mut tx = fixture.owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let row: (Vec<u8>, Vec<u8>, String, i64) = sqlx::query_as(
                "SELECT peer,anonymous_session,board,target FROM verification_private.vip_receipts WHERE token_hash=$1",
            ).bind(receipt.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(
                row,
                (
                    fixture.peers[0].to_vec(),
                    fixture.identifier.to_vec(),
                    fixture.board.clone(),
                    0
                )
            );
            tx.commit().await.unwrap();
            for _ in 0..4 {
                assert_eq!(
                    call(
                        &app,
                        "/v1/vip/receipt",
                        fixture.request("wrong", "198.51.100.31")
                    )
                    .await,
                    (StatusCode::OK, json!({"state":"none"}))
                );
            }
            let mut tx = fixture.owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let failures: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM verification_private.vip_attempts WHERE peer=$1",
            )
            .bind(fixture.peers[1].as_slice())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            assert_eq!(failures, 3);
            tx.commit().await.unwrap();
            sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
                .bind(&fixture.board)
                .execute(&fixture.owner)
                .await
                .unwrap();
            assert_eq!(
                call(
                    &app,
                    "/v1/vip/receipt",
                    fixture.request("owned-vip-password", "203.0.113.31")
                )
                .await,
                (StatusCode::OK, json!({"state":"none"}))
            );
            sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
                .bind(&fixture.board)
                .execute(&fixture.owner)
                .await
                .unwrap();
            let changed = provision_ok(
                &fixture.material,
                json!({"action":"update","identifier":"owner",
                "password":"owned-vip-replacement","name":"Renamed Owned VIP"}),
            );
            assert_eq!(
                call(
                    &app,
                    "/v1/vip/receipt",
                    fixture.request("owned-vip-password", "192.0.2.31")
                )
                .await,
                (StatusCode::OK, json!({"state":"none"}))
            );
            assert_eq!(
                call(
                    &app,
                    "/v1/vip/receipt",
                    fixture.request("owned-vip-replacement", "192.0.2.31")
                )
                .await
                .1["state"],
                "ready"
            );
            let refused = provision(
                &std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap(),
                &fixture.material,
                json!({"action":"update","identifier":"owner","active":false}),
            );
            assert!(!refused.status.success());
            assert!(refused.stdout.is_empty());
            assert_eq!(
                provision_ok(
                    &fixture.material,
                    json!({"action":"update","identifier":"owner","active":false})
                ),
                changed
            );
            assert_eq!(
                call(
                    &app,
                    "/v1/vip/receipt",
                    fixture.request("owned-vip-replacement", "203.0.113.31")
                )
                .await,
                (StatusCode::OK, json!({"state":"none"}))
            );
        }
    };
    let result = tokio::spawn(work).await;
    match Arc::try_unwrap(fixture) {
        Ok(fixture) => fixture.cleanup().await,
        Err(_) => panic!("VIP fixture still retained"),
    }
    result.unwrap();
}

#[tokio::test]
async fn concurrent_vip_attempt_reservations_admit_exactly_three() {
    let fixture = Fixture::new().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = fixture.store.clone();
        let barrier = barrier.clone();
        let identifier = fixture.identifier;
        let peer = fixture.peers[0];
        let board = fixture.board.clone();
        tasks.push(tokio::spawn(async move {
            let ticket = ReceiptToken::generate().unwrap().storage_hash();
            let context = ReceiptToken::generate().unwrap().storage_hash();
            barrier.wait().await;
            store
                .begin_vip_login(&LoginRequest {
                    identifier: &identifier,
                    peer: &peer,
                    ticket: &ticket,
                    anonymous: &identifier,
                    context: &context,
                    board: &board,
                    target: 0,
                    pass_proof: None,
                })
                .await
                .unwrap()
                .status
        }));
    }
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await);
    }
    fixture.cleanup().await;
    let outcomes: Vec<_> = outcomes.into_iter().map(Result::unwrap).collect();
    assert_eq!(
        outcomes.iter().filter(|state| *state == "verify").count(),
        3
    );
    assert_eq!(outcomes.iter().filter(|state| *state == "flood").count(), 5);
}
