use board_config::{MediaAdminSettings, MediaReaderSettings, MonitorSettings, Settings};

#[test]
fn observer_requires_explicit_mode_role_loopback_and_verified_production_tls() {
    for (mode, url) in [
        ("", "postgres://board_monitor:unused@localhost/board"),
        (
            "development",
            "postgres://board_public:unused@localhost/board",
        ),
        (
            "development",
            "postgres://board_monitor:unused@example.com/board",
        ),
        ("development", "postgres://board_monitor:unused@localhost/"),
        (
            "production",
            "postgres://board_monitor:unused@example.com/board",
        ),
        (
            "production",
            "postgres://board_monitor:unused@example.com/board?sslmode=verify-full&sslmode=disable",
        ),
        (
            "production",
            "postgres://board_monitor:unused@example.com/board?sslmode=verify-full&options=-cfoo",
        ),
        (
            "production",
            "postgres://board_monitor:unused@%2Ftmp/board?sslmode=verify-full",
        ),
        (
            "production",
            "postgres://board_monitor@example.com/board?sslmode=verify-full",
        ),
    ] {
        let error = MonitorSettings::parse(mode, url)
            .err()
            .expect("reject invalid configuration");
        assert!(!error.to_string().contains("unused"));
    }
    assert!(
        MonitorSettings::parse(
            "development",
            "postgres://board_monitor:unused@127.0.0.1:55432/board"
        )
        .is_ok()
    );
    assert!(
        MonitorSettings::parse(
            "production",
            "postgres://board_monitor:unused@db.example.com/board?sslmode=verify-full"
        )
        .is_ok()
    );
}

#[test]
fn application_configs_reject_inherited_monitor_credentials_including_non_unicode() {
    for role in ["public", "reader", "writer"] {
        for secret in [
            std::ffi::OsString::from("synthetic-monitor-secret"),
            invalid_unicode(),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .env_clear()
                .args(["--exact", "inherited_monitor_config_child", "--nocapture"])
                .env("APP_ENV", "development")
                .env("MONITOR_CONFIG_CHILD", role)
                .env("MONITOR_DATABASE_URL", secret);
            if let Some(value) = std::env::var_os("SystemRoot") {
                child.env("SystemRoot", value);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{role}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

#[test]
fn inherited_monitor_config_child() {
    let Ok(role) = std::env::var("MONITOR_CONFIG_CHILD") else {
        return;
    };
    let (error, expected) = match role.as_str() {
        "public" => (
            Settings::from_env().err().unwrap(),
            "Operator or staff credentials",
        ),
        "reader" => (
            MediaReaderSettings::from_env().err().unwrap(),
            "must not inherit writer",
        ),
        "writer" => (
            MediaAdminSettings::from_env().err().unwrap(),
            "must not inherit public",
        ),
        _ => panic!("unexpected config child"),
    };
    assert!(error.to_string().contains(expected));
}

fn invalid_unicode() -> std::ffi::OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    }
}
