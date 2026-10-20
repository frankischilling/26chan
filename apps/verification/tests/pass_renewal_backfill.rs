#![cfg(feature = "database-tests")]
use board_domain::{
    challenge_secrets::{PassSessionToken, VerificationKey},
    pass_mail::{Contact, Payload, Recipient},
    verification_mail::MailKey,
};
use std::process::{Command, Stdio};
fn run(database: &str, key: &str, mail: &str) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_board-pass-bind-renewals"));
    cmd.env_clear()
        .env("MIGRATION_DATABASE_URL", database)
        .env("VERIFICATION_KEY", key)
        .env("VERIFICATION_MAIL_KEY", mail)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(value) = std::env::var_os("LD_LIBRARY_PATH") {
        cmd.env("LD_LIBRARY_PATH", value);
    }
    cmd.output().unwrap()
}
#[tokio::test]
async fn expired_and_delayed_legacy_contacts_bind_without_mail_or_credential_changes() {
    let database = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let owner = sqlx::PgPool::connect(&database).await.unwrap();
    let material = PassSessionToken::generate().unwrap().encode();
    let raw = &material[3..];
    let key = VerificationKey::parse(raw).unwrap();
    let mail_material = PassSessionToken::generate().unwrap().encode();
    let raw_mail = &mail_material[3..];
    let mail = MailKey::parse(raw_mail).unwrap();
    let mut rows = Vec::new();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for status in [1_i16, 7] {
        let random = PassSessionToken::generate().unwrap().encode();
        let identifier = random[3..13].to_ascii_uppercase();
        let identity = key.pass_identifier(&identifier).unwrap();
        let reference = format!("owned_backfill_{status}");
        let address = format!("owned+backfill{status}@example.org");
        let contact =
            Contact::new(Recipient::parse(&address).unwrap(), &identifier, &reference).unwrap();
        let recipient = key.pass_recipient(&address).unwrap();
        let encrypted = mail
            .seal_pass(&identity, &Payload::Contact(contact))
            .unwrap();
        let id:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),$2,clock_timestamp()-interval '1 year',CASE WHEN $2=7 THEN NULL ELSE clock_timestamp()-interval '1 day' END) RETURNING id").bind(identity.as_slice()).bind(status).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact) VALUES($1,$2,$3)").bind(id).bind(recipient.as_slice()).bind(encrypted).execute(&mut *tx).await.unwrap();
        rows.push((id, status, reference, identifier, address));
    }
    tx.commit().await.unwrap();
    assert!(
        !run(
            &std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap(),
            raw,
            raw_mail
        )
        .status
        .success()
    );
    assert!(!run(&database, raw, &"00".repeat(32)).status.success());
    let output = run(&database, raw, raw_mail);
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
    assert!(text.contains("Renewal references bound: 2"));
    for (_, _, reference, identifier, address) in &rows {
        for secret in [
            reference.as_str(),
            identifier.as_str(),
            address.as_str(),
            raw,
            raw_mail,
        ] {
            assert!(!text.contains(secret));
        }
    }
    let again = run(&database, raw, raw_mail);
    assert!(again.status.success());
    assert!(String::from_utf8_lossy(&again.stdout).contains("Renewal references bound: 0"));
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for (id, status, reference, _, _) in rows {
        let actual:(i16,i64,i64,Vec<u8>,bool)=sqlx::query_as("SELECT e.status,e.credential_version,c.version,c.renewal_hash,e.pin_hash=repeat('owned-placeholder',3) FROM verification_private.pass_entitlements e JOIN verification_private.pass_contacts c ON c.entitlement_id=e.id WHERE e.id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(
            (actual.0, actual.1, actual.2, actual.4),
            (status, 1, 1, true)
        );
        assert_eq!(actual.3, key.pass_renewal(&reference).unwrap());
        let mail_count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM verification_private.pass_mail_outbox WHERE entitlement_id=$1)+(SELECT count(*) FROM verification_private.pass_recoveries WHERE entitlement_id=$1)").bind(id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(mail_count, 0);
        sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    owner.close().await;
}
