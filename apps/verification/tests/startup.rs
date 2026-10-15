use std::process::Command;
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-verification"));
    command
        .env_clear()
        .arg("--check-config")
        .env("VERIFICATION_BIND", "127.0.0.1:9081")
        .env(
            "VERIFICATION_DATABASE_URL",
            "postgres://board_verifier:owned-private-password@127.0.0.1:55432/imageboard",
        )
        .env("VERIFICATION_SERVICE_TOKEN", "11".repeat(32))
        .env("VERIFICATION_KEY", "22".repeat(32))
        .env("VERIFICATION_MAIL_KEY", "33".repeat(32));
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", root);
    }
    // Dynamic library search is required only for this test binary's platform
    // dependencies; no database or cloud credentials are inherited.
    for name in ["PATH", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}
#[test]
fn config_check_does_not_connect_or_print_credentials() {
    let output = command().output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
#[test]
fn unrelated_authority_and_extra_arguments_fail_without_echoing_secrets() {
    for name in [
        "DATABASE_URL",
        "AUTH_DATABASE_URL",
        "VERIFICATION_MAIL_DATABASE_URL",
        "GH_TOKEN",
        "PUBLIC_VERIFICATION_TOKEN",
    ] {
        let output = command()
            .env(name, "unrelated-private-secret")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8_lossy(&output.stderr);
        assert_eq!(text.trim(), "verification configuration rejected");
        assert!(!text.contains("unrelated-private-secret"));
        assert!(!text.contains("owned-private-password"));
    }
    let output = command()
        .arg("unexpected-private-argument")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "verification arguments rejected"
    );
}
