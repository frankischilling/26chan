#![cfg(feature = "database-tests")]
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_domain::{
    challenge_secrets::{PassSessionToken, VerificationKey},
    verification_mail::MailKey,
};
use board_store::verification::VerificationStore;
use board_verification::{AppState, router};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

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
async fn private_pass_login_checks_credentials_network_holds_flood_and_revocation() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let material = PassSessionToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let identifier = material[3..13].to_ascii_uppercase();
    let token_hash = key.pass_identifier(&identifier).unwrap();
    let flood_peer = key.network_identity("198.51.100.19".parse().unwrap());
    let normal_peer = key.network_identity("192.0.2.19".parse().unwrap());
    let hash = Argon2::default()
        .hash_password(
            b"owned-membership-pin",
            &SaltString::encode_b64(b"owned-pass-fixture").unwrap(),
        )
        .unwrap()
        .to_string();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,6,'2020-01-02T00:00:00Z',clock_timestamp()+interval '1 year') RETURNING id")
  .bind(token_hash.as_slice()).bind(hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let app = router(
        AppState::new(
            store,
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let work = {
        let owner = owner.clone();
        let identifier = identifier.clone();
        async move {
            let (ready_status, ready) = call(&app, "/v1/pass/ready", json!({})).await;
            assert_eq!(ready_status, StatusCode::OK);
            assert_eq!(ready["ready"], true);
            let login = |pin: &str, peer: &str, remember: bool| json!({"identifier":identifier,"pin":pin,"peer":peer,"remember":remember});
            let (status, rejected) =
                call(&app, "/v1/pass/login", login("wrong", "192.0.2.19", false)).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(rejected["state"], "rejected");
            let (_, accepted) = call(
                &app,
                "/v1/pass/login",
                login("owned-membership-pin", "192.0.2.19", false),
            )
            .await;
            assert_eq!(accepted["state"], "ready");
            assert_eq!(accepted["ttl_seconds"], 86400);
            assert!(accepted.get("pin_hash").is_none());
            let token = accepted["token"].as_str().unwrap().to_owned();
            let (_, state) = call(&app, "/v1/pass/state", json!({"token":token})).await;
            assert_eq!(state["state"], "ready");
            assert_eq!(state["purchase_year"], 2020);
            let (_, held) = call(
                &app,
                "/v1/pass/login",
                login("owned-membership-pin", "198.51.100.19", false),
            )
            .await;
            assert_eq!(held["state"], "in_use");
            assert!(
                held["seconds"]
                    .as_u64()
                    .is_some_and(|v| (890..=900).contains(&v))
            );
            let (_, signed_out) = call(&app, "/v1/pass/signout", json!({"token":token})).await;
            assert_eq!(signed_out["state"], "none");
            assert_eq!(
                call(&app, "/v1/pass/state", json!({"token":token})).await.1["state"],
                "none"
            );
            let (_, remembered) = call(
                &app,
                "/v1/pass/login",
                login("owned-membership-pin", "192.0.2.19", true),
            )
            .await;
            assert_eq!(remembered["state"], "ready");
            assert_eq!(remembered["ttl_seconds"], 31556900);
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("UPDATE verification_private.pass_entitlements SET status=4 WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert_eq!(
                call(&app, "/v1/pass/state", json!({"token":remembered["token"]}))
                    .await
                    .1["state"],
                "none"
            );
            let (_, disabled) = call(
                &app,
                "/v1/pass/login",
                login("owned-membership-pin", "192.0.2.19", false),
            )
            .await;
            assert_eq!(disabled, json!({"state":"disabled","status":4}));
            for _ in 0..5 {
                let (_, wrong) = call(
                    &app,
                    "/v1/pass/login",
                    login("wrong", "198.51.100.19", false),
                )
                .await;
                assert_eq!(wrong["state"], "rejected");
            }
            assert_eq!(
                call(
                    &app,
                    "/v1/pass/login",
                    login("wrong", "198.51.100.19", false)
                )
                .await
                .1["state"],
                "flood"
            );
            let response = app
                .clone()
                .oneshot(Request::post("/v1/pass/login").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    };
    let outcome = tokio::spawn(work).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_attempts WHERE peer IN($1,$2)")
        .bind(normal_peer.as_slice())
        .bind(flood_peer.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    owner.close().await;
    outcome.unwrap();
}
