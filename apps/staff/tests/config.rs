use std::process::Command;

#[test]
fn startup_rejects_an_invalid_idle_timeout_before_connecting() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-staff"));
    command.env_clear();
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let output = command
        .env("STAFF_MODE", "development")
        .env("STAFF_ORIGIN", "http://localhost:3001")
        .env("PUBLIC_ORIGIN", "http://localhost:3000")
        .env("MEDIA_ORIGIN", "http://localhost:3002")
        .env("STAFF_BIND", "127.0.0.1:3001")
        .env(
            "AUTH_DATABASE_URL",
            "postgres://board_auth:synthetic@127.0.0.1:55432/imageboard",
        )
        .env(
            "STAFF_DATABASE_URL",
            "postgres://board_staff:synthetic@127.0.0.1:55432/imageboard",
        )
        .env("STAFF_IDLE_TIMEOUT_SECONDS", "malformed")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Invalid STAFF_IDLE_TIMEOUT_SECONDS"));
}
