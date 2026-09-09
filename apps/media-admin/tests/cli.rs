use std::process::Command;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-media-admin"));
    command.env_clear();
    // Windows networking/cryptography requires the OS installation directory.
    if let Some(value) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", value);
    }
    command
}

#[test]
fn production_processing_and_inherited_credentials_are_rejected_before_intake() {
    let output = command()
        .env("APP_ENV", "production")
        .args(["intake", "absent.file", "test.png"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Development intake only"));
    for name in [
        "DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
    ] {
        let output = command()
            .env(name, "synthetic-secret-must-not-appear")
            .arg("status")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("must not inherit"), "{name}: {error}");
        assert!(!error.contains("synthetic-secret-must-not-appear"));
    }
}

#[cfg(feature = "database-tests")]
#[tokio::test]
async fn operator_intake_persists_private_bytes_and_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("quarantine");
    let input = temp.path().join("input.txt");
    std::fs::write(&input, b"synthetic undecoded upload").unwrap();
    let media_url = std::env::var("MEDIA_DATABASE_URL").expect("MEDIA_DATABASE_URL is required");
    let output = command()
        .env("MEDIA_DATABASE_URL", &media_url)
        .env("MEDIA_QUARANTINE_DIR", &root)
        .arg("intake")
        .arg(&input)
        .arg("untrusted/display.png")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let id = result["id"].as_str().unwrap();
    assert_eq!(result["state"], "queued");
    assert_eq!(
        std::fs::read(root.join(format!("{id}.input"))).unwrap(),
        b"synthetic undecoded upload"
    );
    let output = command()
        .env("MEDIA_DATABASE_URL", &media_url)
        .arg("status")
        .arg(id)
        .output()
        .unwrap();
    assert!(output.status.success());
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["state"], "queued");
    assert!(status.get("lease_token").is_none());
    assert!(status.get("filename").is_none());
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media.jobs SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1",
    )
    .bind(id)
    .execute(&admin)
    .await
    .unwrap();
    let output = command()
        .env("MEDIA_DATABASE_URL", &media_url)
        .env("MEDIA_QUARANTINE_DIR", &root)
        .arg("cleanup")
        .output()
        .unwrap();
    assert!(output.status.success());
    let state: String = sqlx::query_scalar("SELECT state FROM media.jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(state, "failed");
    sqlx::query(
        "UPDATE media.jobs SET updated_at = clock_timestamp() - interval '2 days' WHERE id = $1",
    )
    .bind(id)
    .execute(&admin)
    .await
    .unwrap();
    let output = command()
        .env("MEDIA_DATABASE_URL", &media_url)
        .env("MEDIA_QUARANTINE_DIR", &root)
        .arg("cleanup")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!root.join(format!("{id}.input")).exists());
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    admin.close().await;
}
