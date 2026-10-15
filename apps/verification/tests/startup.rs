use std::process::Command;
fn command() -> Command {
    command_with_mode("--check-config")
}
fn command_with_mode(mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-verification"));
    command
        .env_clear()
        .arg(mode)
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

#[test]
fn operator_recipient_hash_is_bounded_and_never_connects_or_echoes_the_address() {
    use std::{io::Write, process::Stdio};
    for (input, accepted) in [
        ("owned.operator.fixture@gmail.com\n".to_owned(), true),
        ("not-an-email".into(), false),
        ("x".repeat(321), false),
    ] {
        let mut child = command_with_mode("--recipient-hash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.success(), accepted);
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&input));
        if accepted {
            let prepared =
                board_domain::email_verification::PreparedEmail::prepare(&input).unwrap();
            let expected: String =
                board_domain::challenge_secrets::VerificationKey::parse(&"22".repeat(32))
                    .unwrap()
                    .email_recipient(&prepared)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
            assert!(output.stderr.is_empty());
        } else {
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8_lossy(&output.stderr).trim(),
                "verification recipient rejected"
            );
        }
    }
}
