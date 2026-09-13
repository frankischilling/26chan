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
fn invalid_request_budgets_fail_before_binding_or_database_access() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let command = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"));
        command
            .env_clear()
            .env(
                "DATABASE_URL",
                "postgres://board_public:unused-secret@127.0.0.1:1/absent",
            )
            .env("BIND_ADDR", occupied.local_addr().unwrap().to_string());
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        command
    };
    for name in board_config::PublicRequestLimits::NAMES {
        for value in [
            "",
            "0",
            "999999999999999999999999",
            "untrusted-input-marker",
        ] {
            let output = command().env(name, value).output().unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains(name), "{error}");
            assert!(!error.contains("unused-secret"));
            assert!(!error.contains("untrusted-input-marker"));
            assert!(!error.contains("AddrInUse"));
        }
    }
    // A valid configuration reaches the same healthy, already-owned listener.
    let output = command().output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AddrInUse"));
}

#[test]
fn complete_development_media_configuration_never_enables_production() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
        .env_clear()
        .env("APP_ENV", "production")
        .env("MEDIA_ENABLED", "true")
        .env("PUBLIC_MEDIA_PROFILE", "isolated-development")
        .env("PUBLIC_INTAKE_ADDR", "127.0.0.1:3004")
        .env("PUBLIC_INTAKE_TOKEN", "a".repeat(64))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Media processing is unavailable"));
    assert!(!error.contains(&"a".repeat(64)));
}

#[test]
fn partial_or_unscoped_intake_configuration_fails_before_database_access() {
    for (key, value) in [
        ("PUBLIC_INTAKE_TOKEN", "synthetic-secret"),
        ("PUBLIC_INTAKE_ADDR", "127.0.0.1:3004"),
        ("PUBLIC_MEDIA_PROFILE", "isolated-development"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
            .env_clear()
            .env(key, value)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("explicit development media enablement"));
        assert!(!error.contains("synthetic-secret"));
    }
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
    for name in ["MEDIA_DATABASE_URL", "MEDIA_READ_DATABASE_URL"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"))
            .env_clear()
            .env(name, "synthetic-media-secret")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("media credentials"));
        assert!(!error.contains("synthetic-media-secret"));
    }
}

#[test]
fn invalid_api_listener_configuration_fails_before_database_access() {
    for (origin, bind) in [
        (Some("http://127.0.0.1:3003"), None),
        (None, Some("127.0.0.1:3003")),
        (Some("http://127.0.0.1:3000"), Some("127.0.0.1:3003")),
        (Some("http://localhost:3001"), Some("127.0.0.1:3003")),
        (Some("http://127.0.0.1:3002"), Some("127.0.0.1:3003")),
        (Some("http://127.0.0.1:3003/path"), Some("127.0.0.1:3003")),
        (Some("http://example.com"), Some("127.0.0.1:3003")),
        (Some("http://127.0.0.1:3003"), Some("0.0.0.0:3003")),
        (Some("http://127.0.0.1:3003"), Some("127.0.0.1:3000")),
        (Some("http://127.0.0.1:3003"), Some("not-an-address")),
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"));
        command
            .env_clear()
            .env("STAFF_ORIGIN", "http://localhost:3001")
            .env(
                "DATABASE_URL",
                "postgres://board_public:unused@127.0.0.1:1/absent",
            );
        if let Some(origin) = origin {
            command.env("API_ORIGIN", origin);
        }
        if let Some(bind) = bind {
            command.env("API_BIND_ADDR", bind);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("API"),
            "origin={origin:?} bind={bind:?}: {error}"
        );
        assert!(!error.contains("unused"));
    }
}

#[test]
fn occupied_api_listener_fails_before_database_access() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let public = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let public_bind = public.local_addr().unwrap();
    drop(public);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"));
    command.env_clear();
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let output = command
        .env(
            "DATABASE_URL",
            "postgres://board_public:unused@127.0.0.1:1/absent",
        )
        .env("BIND_ADDR", public_bind.to_string())
        .env("API_ORIGIN", "http://127.0.0.1:3003")
        .env("API_BIND_ADDR", occupied.local_addr().unwrap().to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AddrInUse"));
    // Neither listener serves if acquiring the pair fails.
    assert!(std::net::TcpListener::bind(public_bind).is_ok());
}

#[test]
fn metrics_startup_rejects_bad_config_and_occupied_socket_before_database_access() {
    let database = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    database.set_nonblocking(true).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    for token in ["synthetic-secret".to_owned(), "a".repeat(64)] {
        let public = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let public_address = public.local_addr().unwrap();
        drop(public);
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"));
        command.env_clear();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        let output = command
            .env(
                "DATABASE_URL",
                format!(
                    "postgres://board_public:unused@{}/absent",
                    database.local_addr().unwrap()
                ),
            )
            .env("BIND_ADDR", public_address.to_string())
            .env(
                "METRICS_BIND_ADDR",
                occupied.local_addr().unwrap().to_string(),
            )
            .env("METRICS_TOKEN", &token)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains(&token));
        assert!(!error.contains("unused"));
        assert!(std::net::TcpListener::bind(public_address).is_ok());
        assert_eq!(
            database.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
