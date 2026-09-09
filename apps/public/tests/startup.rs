#[test]
fn media_cannot_be_enabled_even_without_database_configuration() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
        .env_clear()
        .env("MEDIA_ENABLED", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Media processing is unavailable"));
}

#[test]
fn public_runtime_rejects_inherited_operator_credentials() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
        .env_clear()
        .env("MIGRATION_DATABASE_URL", "synthetic-operator-secret")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Operator or staff credentials"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-operator-secret"));
}

#[test]
fn public_runtime_rejects_the_media_queue_credential() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
        .env_clear()
        .env("MEDIA_DATABASE_URL", "synthetic-media-secret")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("media credentials"));
    assert!(!error.contains("synthetic-media-secret"));
}
