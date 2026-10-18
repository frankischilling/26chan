use std::process::Command;
#[test]
fn compatibility_key_is_optional_separate_and_errors_do_not_echo_it() {
    for (value, valid, present) in [
        (None, true, false),
        (Some("11".repeat(64)), true, true),
        (Some("".into()), false, false),
        (Some("private-invalid-material".into()), false, false),
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .env_clear()
            .args(["--exact", "posting_browser_config_child", "--nocapture"]);
        #[cfg(windows)]
        for key in ["PATH", "SystemRoot"] {
            if let Some(value) = std::env::var_os(key) {
                child.env(key, value);
            }
        }
        child
            .env("POSTING_BROWSER_CONFIG_CHILD", "1")
            .env("EXPECTED_VALID", if valid { "1" } else { "0" })
            .env("EXPECTED_PRESENT", if present { "1" } else { "0" })
            .env("APP_ENV", "development")
            .env(
                "DATABASE_URL",
                "postgres://board_public:unused@127.0.0.1:55432/imageboard",
            );
        if let Some(value) = value {
            child.env("POSTING_BROWSER_COMPAT_KEY", value);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-invalid-material"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-invalid-material"));
    }
}
#[test]
fn posting_browser_config_child() {
    if std::env::var_os("POSTING_BROWSER_CONFIG_CHILD").is_none() {
        return;
    }
    let result = board_config::Settings::from_env();
    assert_eq!(
        result.is_ok(),
        std::env::var("EXPECTED_VALID").unwrap() == "1"
    );
    match result {
        Ok(settings) => {
            assert_eq!(
                settings.posting_browser_compat_key.is_some(),
                std::env::var("EXPECTED_PRESENT").unwrap() == "1"
            );
            assert!(settings.browser_hint_key.is_none());
        }
        Err(error) => assert!(!error.to_string().contains("private-invalid-material")),
    }
}
