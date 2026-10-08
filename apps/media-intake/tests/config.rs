use std::{ffi::OsString, process::Command};

#[test]
fn intake_configuration_rejects_unsafe_or_ambiguous_inputs_without_echoing_secrets() {
    for (name, value) in [
        ("MEDIA_INTAKE_MODE", "production"),
        ("MEDIA_INTAKE_BIND", "0.0.0.0:3900"),
        (
            "INTAKE_DATABASE_URL",
            "postgres://board_media_intake:secret@127.0.0.1/imageboard?sslmode=disable",
        ),
        ("MEDIA_QUARANTINE_DIR", "relative-quarantine"),
        ("MEDIA_INTAKE_TOKEN", "not-a-token"),
        ("MEDIA_INTAKE_STAFF_TOKEN", ""),
        ("MEDIA_INTAKE_STAFF_TOKEN", "not-a-token"),
        ("MEDIA_INTAKE_STAFF_TOKEN", &"A".repeat(64)),
        ("MEDIA_INTAKE_STAFF_TOKEN", &"b".repeat(63)),
        ("MEDIA_INTAKE_STAFF_TOKEN", &"a".repeat(64)),
        ("STAFF_INTAKE_TOKEN", "synthetic-unrelated-secret"),
        ("DATABASE_URL", "synthetic-unrelated-secret"),
        ("database_url", "synthetic-unrelated-secret"),
        ("MEDIA_DISPATCH_CONFIG", "synthetic-unrelated-secret"),
        ("AWS_SESSION_TOKEN", "synthetic-unrelated-secret"),
        ("METRICS_TOKEN", &"a".repeat(64)),
    ] {
        let output = child_with(name, value).output().unwrap();
        assert!(!output.status.success(), "{name} accepted");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!text.contains("secret"), "{name} leaked a secret");
    }
}

#[test]
fn intake_configuration_accepts_only_explicit_development_loopback_values() {
    let output = child_with("MEDIA_INTAKE_MODE", "development")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn intake_configuration_accepts_a_distinct_optional_staff_service_token() {
    let output = child_with("MEDIA_INTAKE_STAFF_TOKEN", &"b".repeat(64))
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn optional_staff_token_cannot_replace_public_token_or_reuse_metrics_token() {
    let staff = "b".repeat(64);
    let mut missing_public = child_with("MEDIA_INTAKE_STAFF_TOKEN", &staff);
    missing_public.env_remove("MEDIA_INTAKE_TOKEN");
    assert!(!missing_public.output().unwrap().status.success());
    let output = child_with("MEDIA_INTAKE_STAFF_TOKEN", &staff)
        .env("METRICS_TOKEN", &staff)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&staff));
}

#[cfg(unix)]
#[test]
fn non_unicode_staff_token_is_rejected_instead_of_disabling_the_slot() {
    use std::os::unix::ffi::OsStringExt;
    let output = child_with("MEDIA_INTAKE_MODE", "development")
        .env("MEDIA_INTAKE_STAFF_TOKEN", OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert!(!output.status.success());
}

fn child_with(name: &str, value: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-media-intake"));
    command.env_clear();
    command.arg("--check-config");
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
        .env("MEDIA_INTAKE_MODE", "development")
        .env("MEDIA_INTAKE_BIND", "127.0.0.1:3900")
        .env(
            "INTAKE_DATABASE_URL",
            "postgres://board_media_intake:synthetic@127.0.0.1:55432/imageboard",
        )
        .env("MEDIA_QUARANTINE_DIR", std::env::temp_dir())
        .env("MEDIA_INTAKE_TOKEN", "a".repeat(64))
        .env(name, OsString::from(value));
    command
}
