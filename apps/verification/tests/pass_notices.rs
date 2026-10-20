#![cfg(feature = "database-tests")]
use board_domain::{
    challenge_secrets::{PassSessionToken, VerificationKey},
    pass_mail::Payload,
    verification_mail::MailKey,
};
use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::PgPool;
use std::{
    io::Write,
    process::{Command, Stdio},
};
fn command(binary: &str, vars: &[(&str, &str)], input: Option<String>) -> std::process::Output {
    let mut cmd = Command::new(binary);
    cmd.env_clear().envs(vars.iter().copied());
    if let Some(value) = std::env::var_os("LD_LIBRARY_PATH") {
        cmd.env("LD_LIBRARY_PATH", value);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}
#[tokio::test]
async fn operator_contact_and_notice_producer_keep_secrets_private_and_queue_once() {
    let database = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let verifier = std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap();
    let owner = PgPool::connect(&database).await.unwrap();
    let material = PassSessionToken::generate().unwrap().encode();
    let mail_material = PassSessionToken::generate().unwrap().encode();
    let raw = &material[3..];
    let raw_mail = &mail_material[3..];
    let identifier = material[3..13].to_ascii_uppercase();
    let key = VerificationKey::parse(raw).unwrap();
    let hash = key.pass_identifier(&identifier).unwrap();
    let mail = MailKey::parse(raw_mail).unwrap();
    let vars = [
        ("MIGRATION_DATABASE_URL", database.as_str()),
        ("VERIFICATION_KEY", raw),
        ("VERIFICATION_MAIL_KEY", raw_mail),
    ];
    let create=command(env!("CARGO_BIN_EXE_board-pass-admin"),&vars,Some(json!({"action":"create","identifier":identifier,"pin":"owned-notice-pin","status":6,"purchased_at":"2020-01-02T00:00:00Z","expires_at":Utc::now()+Duration::days(6)}).to_string()));
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    let contact=command(env!("CARGO_BIN_EXE_board-pass-admin"),&vars,Some(json!({"action":"contact","identifier":identifier,"email":"buyer@example.org","gift_email":"Gift+Owner@example.org","renewal_reference":"owned_reference"}).to_string()));
    assert!(
        contact.status.success(),
        "{}",
        String::from_utf8_lossy(&contact.stderr)
    );
    let producer_vars = [
        ("VERIFICATION_DATABASE_URL", verifier.as_str()),
        ("VERIFICATION_KEY", raw),
        ("VERIFICATION_MAIL_KEY", raw_mail),
    ];
    for _ in 0..2 {
        let output = command(
            env!("CARGO_BIN_EXE_board-pass-notices"),
            &producer_vars,
            None,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for secret in [
            identifier.as_str(),
            raw,
            raw_mail,
            "Gift+Owner@example.org",
            "owned_reference",
        ] {
            assert!(!text.contains(secret));
        }
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let rows:Vec<(Vec<u8>,Vec<u8>)>=sqlx::query_as("SELECT q.token_hash,q.encrypted_payload FROM verification_private.pass_mail_outbox q JOIN verification_private.pass_entitlements e ON e.id=q.entitlement_id WHERE e.token_hash=$1").bind(hash.as_slice()).fetch_all(&mut *tx).await.unwrap();
    let stored:Vec<u8>=sqlx::query_scalar("SELECT c.encrypted_contact FROM verification_private.pass_contacts c JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE e.token_hash=$1").bind(hash.as_slice()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE token_hash=$1")
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    owner.close().await;
    assert_eq!(rows.len(), 1);
    let Payload::Contact(contact) = mail.open_pass(&hash, &stored).unwrap() else {
        panic!("contact envelope expected")
    };
    assert_eq!(contact.recipient(), "Gift+Owner@example.org");
    let context: [u8; 32] = rows[0].0.as_slice().try_into().unwrap();
    let Payload::Notice { contact, kind, .. } = mail.open_pass(&context, &rows[0].1).unwrap()
    else {
        panic!("notice envelope expected")
    };
    assert_eq!(kind, board_domain::pass_mail::Notice::Reminder);
    assert_eq!(contact.identifier(), identifier);
    assert_eq!(contact.recipient(), "Gift+Owner@example.org");
}
