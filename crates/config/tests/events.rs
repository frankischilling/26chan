use std::process::Command;
#[test]
fn event_workflows_require_explicit_activation_and_a_key() {
    for (flag, key, valid, enabled) in [
        (None, false, true, false),
        (Some("false"), false, true, false),
        (Some("true"), false, false, false),
        (Some("true"), true, true, true),
        (Some("false"), true, true, false),
        (Some("yes"), true, false, false),
        (Some("TRUE"), true, false, false),
        (Some(""), true, false, false),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .env_clear()
            .args(["--exact", "event_config_child", "--nocapture"]);
        #[cfg(windows)]
        for key in ["PATH", "SystemRoot"] {
            if let Some(value) = std::env::var_os(key) {
                child.env(key, value);
            }
        }
        child
            .env("EVENT_CONFIG_CHILD", "1")
            .env("EXPECTED_VALID", if valid { "1" } else { "0" })
            .env("EXPECTED_ENABLED", if enabled { "1" } else { "0" })
            .env("APP_ENV", "development")
            .env(
                "DATABASE_URL",
                "postgres://board_public:owned@127.0.0.1:9/unavailable",
            );
        if let Some(flag) = flag {
            child.env("PUBLIC_EVENT_WORKFLOWS", flag);
        }
        if key {
            child.env("POSTER_ID_KEY", "4".repeat(64));
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn event_config_child() {
    if std::env::var_os("EVENT_CONFIG_CHILD").is_none() {
        return;
    }
    let result = board_config::Settings::from_env();
    assert_eq!(
        result.is_ok(),
        std::env::var("EXPECTED_VALID").unwrap() == "1"
    );
    if let Ok(settings) = result {
        assert_eq!(
            settings.event_workflows,
            std::env::var("EXPECTED_ENABLED").unwrap() == "1"
        );
    }
}
