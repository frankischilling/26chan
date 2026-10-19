#![cfg(feature = "database-tests")]
use board_domain::challenge_secrets::{PassSessionToken, VerificationKey};
use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn run(database: &str, key: &str, input: Value) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-pass-admin"));
    command
        .env_clear()
        .env("MIGRATION_DATABASE_URL", database)
        .env("VERIFICATION_KEY", key);
    if let Some(value) = std::env::var_os("LD_LIBRARY_PATH") {
        command.env("LD_LIBRARY_PATH", value);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}
#[tokio::test]
async fn operator_provisions_and_updates_only_owned_membership_without_printing_secrets() {
    let database = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let owner = PgPool::connect(&database).await.unwrap();
    let material = PassSessionToken::generate().unwrap().encode();
    let key = &material[3..];
    let identifier = material[3..13].to_ascii_uppercase();
    let hash = VerificationKey::parse(key)
        .unwrap()
        .pass_identifier(&identifier)
        .unwrap();
    let expires = Utc::now() + Duration::days(365);
    let renewed = expires + Duration::days(365);
    let work = {
        let database = database.clone();
        let material = material.clone();
        let identifier = identifier.clone();
        async move {
            let key = &material[3..];
            let operations = [
                json!({"action":"create","identifier":identifier,"pin":"owned-provision-pin","status":6,"purchased_at":"2020-01-02T00:00:00Z","expires_at":expires}),
                json!({"action":"reset_pin","identifier":identifier,"pin":"owned-reset-pin"}),
                json!({"action":"status","identifier":identifier,"status":4}),
                json!({"action":"renew","identifier":identifier,"expires_at":renewed}),
            ];
            let mut id = None;
            for input in operations {
                let output = run(&database, key, input);
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(!text.contains(&identifier));
                assert!(!text.contains("owned-provision-pin"));
                assert!(!text.contains("owned-reset-pin"));
                assert!(!text.contains("argon2"));
                let value: Value = serde_json::from_str(&text).unwrap();
                if let Some(id) = id {
                    assert_eq!(value["id"].as_i64().unwrap(), id)
                } else {
                    id = value["id"].as_i64();
                }
            }
            assert!(
                !run(
                    &database,
                    key,
                    json!({"action":"renew","identifier":identifier,"expires_at":expires})
                )
                .status
                .success()
            );
            assert!(
                !run(
                    &std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap(),
                    key,
                    json!({"action":"status","identifier":identifier,"status":4})
                )
                .status
                .success()
            );
            assert!(!run(&database,key,json!({"action":"reset_pin","identifier":identifier,"pin":"owned-reset-pin","extra":"refuse"})).status.success());
        }
    };
    let outcome = tokio::spawn(work).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let row:Option<(i16,i64,String)>=sqlx::query_as("SELECT status,credential_version,pin_hash FROM verification_private.pass_entitlements WHERE token_hash=$1").bind(hash.as_slice()).fetch_optional(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE token_hash=$1")
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    owner.close().await;
    outcome.unwrap();
    let (status, version, pin_hash) = row.unwrap();
    assert_eq!(status, 0);
    assert_eq!(version, 4);
    assert!(pin_hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
}
