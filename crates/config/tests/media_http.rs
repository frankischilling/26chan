use board_config::MediaHttpSettings;
use std::process::Command;

#[test]
fn publication_group_read_is_explicit_and_typed() {
    for value in ["false", "true", "invalid"] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "media_group_config_child", "--nocapture"])
            .env_clear();
        if let Some(value) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", value);
        }
        let result = command
            .env("MEDIA_GROUP_READ", value)
            .env("MEDIA_GROUP_CONFIG_CHILD", "1")
            .env("APP_ENV", "development")
            .env(
                "MEDIA_DATABASE_URL",
                "postgres://board_media:synthetic@127.0.0.1/test",
            )
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "group case {value}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

#[test]
fn media_group_config_child() {
    if std::env::var("MEDIA_GROUP_CONFIG_CHILD").is_err() {
        return;
    }
    let value = std::env::var("MEDIA_GROUP_READ").unwrap();
    let settings = board_config::MediaAdminSettings::from_env();
    if value == "invalid" {
        assert!(settings.is_err());
    } else {
        assert_eq!(settings.unwrap().group_read, value == "true");
    }
}

#[test]
fn http_reader_rejects_unsafe_configuration_before_connecting() {
    for (name, value, valid) in [
        ("", "", true),
        ("APP_ENV", "production", false),
        ("APP_ENV", "", false),
        ("MEDIA_ENABLED", "true", false),
        ("MEDIA_APPROVED_DIR", "relative", false),
        ("MEDIA_APPROVED_DIR", "", false),
        ("MEDIA_BIND_ADDR", "0.0.0.0:3002", false),
        ("MEDIA_BIND_ADDR", "127.0.0.1:0", false),
        ("MEDIA_BIND_ADDR", "127.0.0.1:3100", false),
        ("MEDIA_ORIGIN", "http://127.0.0.1:3000", false),
        ("MEDIA_ORIGIN", "https://127.0.0.1:3002", false),
        ("MEDIA_ORIGIN", "http://images.example.net", false),
        ("MEDIA_ORIGIN", "http://127.0.0.1:3002/path", false),
        ("API_ORIGIN", "http://127.0.0.1:3002", false),
        ("API_ORIGIN", "http://127.0.0.1:3003", true),
        ("DATABASE_URL", "synthetic-other-credential", false),
        ("MEDIA_DATABASE_URL", "synthetic-other-credential", false),
        (
            "MIGRATION_DATABASE_URL",
            "synthetic-other-credential",
            false,
        ),
        ("STAFF_DATABASE_URL", "synthetic-other-credential", false),
        ("AUTH_DATABASE_URL", "synthetic-other-credential", false),
        (
            "MEDIA_READ_DATABASE_URL",
            "postgres://board_media@127.0.0.1/test",
            false,
        ),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "media_http_config_child", "--nocapture"])
            .env_clear();
        if let Some(value) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", value);
        }
        command
            .env(
                "MEDIA_HTTP_CONFIG_CASE",
                if valid { "valid" } else { "invalid" },
            )
            .env("APP_ENV", "development")
            .env(
                "MEDIA_READ_DATABASE_URL",
                "postgres://board_media_read:synthetic@127.0.0.1:55432/test",
            )
            .env("MEDIA_APPROVED_DIR", std::env::temp_dir())
            .env("MEDIA_ORIGIN", "http://127.0.0.1:3002")
            .env("MEDIA_BIND_ADDR", "127.0.0.1:3002");
        if !name.is_empty() {
            command.env(name, value);
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "case {name}: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

#[test]
fn media_http_config_child() {
    let Ok(case) = std::env::var("MEDIA_HTTP_CONFIG_CASE") else {
        return;
    };
    let settings = MediaHttpSettings::from_env();
    assert_eq!(settings.is_ok(), case == "valid");
    if let Ok(settings) = settings {
        assert_eq!(settings.bind.port(), 3002);
        assert!(settings.approved_dir.is_absolute());
        assert_eq!(settings.origin.as_string(), "http://127.0.0.1:3002");
    }
}
