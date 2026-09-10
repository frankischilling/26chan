use std::process::Command;

#[test]
fn staff_rejects_inherited_media_reader_credentials() {
    for credential in ["MEDIA_READ_DATABASE_URL", "MONITOR_DATABASE_URL"] {
        let output = Command::new(env!("CARGO_BIN_EXE_board-staff"))
            .env_clear()
            .env(credential, "synthetic-reader-secret")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("unrelated database credential"));
        assert!(!error.contains("synthetic-reader-secret"));
    }
}

#[test]
fn staff_rejects_non_unicode_monitor_credentials() {
    #[cfg(unix)]
    let secret = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    #[cfg(windows)]
    let secret = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    let output = Command::new(env!("CARGO_BIN_EXE_board-staff"))
        .env_clear()
        .env("MONITOR_DATABASE_URL", secret)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrelated database credential"));
}

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

#[test]
fn staff_metrics_failure_does_not_connect_stores_or_leave_staff_serving() {
    let database = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    database.set_nonblocking(true).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    for token in ["synthetic-secret".to_owned(), "a".repeat(64)] {
        let staff = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let staff_address = staff.local_addr().unwrap();
        drop(staff);
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
            .env("STAFF_BIND", staff_address.to_string())
            .env(
                "AUTH_DATABASE_URL",
                format!(
                    "postgres://board_auth:unused@{}/absent",
                    database.local_addr().unwrap()
                ),
            )
            .env(
                "STAFF_DATABASE_URL",
                format!(
                    "postgres://board_staff:unused@{}/absent",
                    database.local_addr().unwrap()
                ),
            )
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
        assert!(std::net::TcpListener::bind(staff_address).is_ok());
        assert_eq!(
            database.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
