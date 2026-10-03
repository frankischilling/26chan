fn public_command() -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-public"));
    command.env_clear();
    // Windows loads the ICU runtime before Rust main. Retain loader paths,
    // while application configuration and credentials remain explicit below.
    #[cfg(windows)]
    for key in ["PATH", "SystemRoot"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
}

#[test]
fn media_cannot_be_enabled_even_without_database_configuration() {
    let output = public_command()
        .env("MEDIA_ENABLED", "true")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Media processing is unavailable"));
}

#[test]
fn invalid_public_identity_keys_fail_before_binding_without_echoing_secrets() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    for variable in ["TRIPCODE_KEY", "POSTER_ID_KEY"] {
        for key in [
            "".to_owned(),
            "owned-invalid-secret".into(),
            "0".repeat(64),
            "g".repeat(64),
        ] {
            let mut command = public_command();
            command
                .env(variable, &key)
                .env(
                    "DATABASE_URL",
                    "postgres://board_public:unused@127.0.0.1:1/absent",
                )
                .env("BIND_ADDR", occupied.local_addr().unwrap().to_string());
            let output = command.output().unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains(variable), "{error}");
            assert!(!error.contains("owned-invalid-secret"));
            assert!(!error.contains("AddrInUse"));
        }
    }
}

#[test]
fn invalid_request_budgets_fail_before_binding_or_database_access() {
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let command = || {
        let mut command = public_command();
        command
            .env(
                "DATABASE_URL",
                "postgres://board_public:unused-secret@127.0.0.1:1/absent",
            )
            .env("BIND_ADDR", occupied.local_addr().unwrap().to_string());
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
    let output = command()
        .env("PUBLIC_MAX_RESPONSE_BUFFER_BYTES", "4095")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("PUBLIC_MAX_RESPONSE_BUFFER_BYTES"));
    assert!(!error.contains("unused-secret"));
    assert!(!error.contains("AddrInUse"));

    // The minimum aggregate output pool is valid and reaches the same healthy,
    // already-owned listener instead of failing configuration validation.
    let output = command()
        .env("PUBLIC_MAX_RESPONSE_BUFFER_BYTES", "4096")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AddrInUse"));

    // A valid configuration reaches the same healthy, already-owned listener.
    let output = command().output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("AddrInUse"));
}

#[test]
fn complete_development_media_configuration_never_enables_production() {
    let output = public_command()
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
        let output = public_command().env(key, value).output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("explicit development media enablement"));
        assert!(!error.contains("synthetic-secret"));
    }
}

#[test]
fn public_runtime_rejects_inherited_operator_credentials() {
    for name in [
        "MIGRATION_DATABASE_URL",
        "STAFF_POSTER_ID_KEY",
        "STAFF_COUNTRY_DATABASE",
        "STAFF_PROXY_SOCKET",
        "STAFF_PROXY_UID",
    ] {
        let output = public_command()
            .env(name, "synthetic-operator-secret")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Operator or staff credentials"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-operator-secret"));
    }
}

#[test]
fn public_runtime_rejects_the_media_queue_credential() {
    for name in ["MEDIA_DATABASE_URL", "MEDIA_READ_DATABASE_URL"] {
        let output = public_command()
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
        let mut command = public_command();
        command.env("STAFF_ORIGIN", "http://localhost:3001").env(
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
    let mut command = public_command();
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
    for (token, expected) in [
        ("synthetic-secret".to_owned(), "ConfigError"),
        ("a".repeat(64), "AddrInUse"),
    ] {
        let public = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let public_address = public.local_addr().unwrap();
        drop(public);
        let mut command = public_command();
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
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains(&token));
        assert!(!error.contains("unused"));
        assert!(std::net::TcpListener::bind(public_address).is_ok());
        assert_eq!(
            database.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
