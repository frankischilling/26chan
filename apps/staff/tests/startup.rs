use std::{path::Path, process::Command};

fn staff_command(bind: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-staff"));
    command.env_clear();
    #[cfg(windows)]
    for key in ["PATH", "SystemRoot"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("STAFF_MODE", "development")
        .env("STAFF_ORIGIN", "http://localhost:3001")
        .env("PUBLIC_ORIGIN", "http://127.0.0.1:3000")
        .env("MEDIA_ORIGIN", "http://127.0.0.1:3002")
        .env("STAFF_BIND", bind)
        .env(
            "AUTH_DATABASE_URL",
            "postgres://board_auth:owned-auth-secret@127.0.0.1:1/absent",
        )
        .env(
            "STAFF_DATABASE_URL",
            "postgres://board_staff:owned-staff-secret@127.0.0.1:1/absent",
        );
    command
}

fn failure(command: &mut Command) -> String {
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    for private in [
        "owned-auth-secret",
        "owned-staff-secret",
        "owned-invalid-secret",
        "owned-private-source",
    ] {
        assert!(!error.contains(private), "Startup exposed private input.");
    }
    error
}

#[test]
fn invalid_staff_identity_sources_fail_before_listener_binding() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = occupied.local_addr().unwrap().to_string();
    for variable in ["STAFF_TRIPCODE_KEY", "STAFF_POSTER_ID_KEY"] {
        for key in [
            "".to_owned(),
            "owned-invalid-secret".into(),
            "0".repeat(64),
            "g".repeat(64),
        ] {
            let error = failure(staff_command(&bind).env(variable, key));
            assert!(error.contains(variable));
            assert!(!error.contains("Staff listener unavailable"));
        }
    }
    for path in [
        std::path::PathBuf::from("owned-private-source.mmdb"),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("owned-private-source.mmdb"),
    ] {
        let error = failure(staff_command(&bind).env("STAFF_COUNTRY_DATABASE", path));
        assert!(error.contains("Invalid STAFF_COUNTRY_DATABASE"));
        assert!(!error.contains("Staff listener unavailable"));
    }
    let source = tempfile::NamedTempFile::new().unwrap();
    source.as_file().set_len(64 * 1024 * 1024 + 1).unwrap();
    let error = failure(staff_command(&bind).env("STAFF_COUNTRY_DATABASE", source.path()));
    assert!(error.contains("Invalid STAFF_COUNTRY_DATABASE"));
    assert!(!error.contains(source.path().to_str().unwrap()));
    assert!(!error.contains("Staff listener unavailable"));
}

#[test]
fn dedicated_staff_sources_validate_and_reach_the_owned_listener() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = occupied.local_addr().unwrap().to_string();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/domain/tests/fixtures/GeoIP2-Country-Test.mmdb");
    for configured in [false, true] {
        let mut command = staff_command(&bind);
        if configured {
            command
                .env("STAFF_TRIPCODE_KEY", "12".repeat(32))
                .env("STAFF_POSTER_ID_KEY", "34".repeat(32))
                .env("STAFF_COUNTRY_DATABASE", &source);
        }
        let error = failure(&mut command);
        assert!(error.contains("Staff listener unavailable"));
        assert!(!error.contains(&"12".repeat(32)));
        assert!(!error.contains(&"34".repeat(32)));
        assert!(!error.contains(source.to_str().unwrap()));
    }
}

#[test]
fn public_identity_sources_cannot_be_inherited_by_the_staff_runtime() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = occupied.local_addr().unwrap().to_string();
    for variable in [
        "MIGRATION_DATABASE_URL",
        "DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "MONITOR_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "PUBLIC_INTAKE_TOKEN",
        "TRIPCODE_KEY",
        "POSTER_ID_KEY",
        "COUNTRY_DATABASE",
        "PUBLIC_PROXY_SOCKET",
        "PUBLIC_PROXY_UID",
    ] {
        let error = failure(
            staff_command(&bind)
                .env("STAFF_POSTER_ID_KEY", "34".repeat(32))
                .env(variable, "owned-invalid-secret"),
        );
        assert!(error.contains(
            "Staff runtime received an unrelated database credential or identity source"
        ));
        assert!(!error.contains("Staff listener unavailable"));
    }
}
