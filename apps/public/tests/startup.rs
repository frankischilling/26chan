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
