use board_config::{MediaAdminSettings, MediaReaderSettings, Settings};
use std::process::Command;

#[test]
fn separated_media_configuration_is_checked_before_connecting() {
    for case in [
        "valid",
        "missing-mode",
        "production",
        "wrong-role",
        "remote",
        "writer",
        "public",
        "owner",
        "staff",
        "auth",
        "test-public",
        "enabled",
        "public-inherits-reader",
        "writer-inherits-reader",
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "media_reader_config_child", "--nocapture"])
            .env_clear();
        if let Some(value) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", value);
        }
        command
            .env("BOARD_READER_CONFIG_CASE", case)
            .env("APP_ENV", "development")
            .env(
                "MEDIA_READ_DATABASE_URL",
                "postgres://board_media_read:synthetic@127.0.0.1:55432/imageboard",
            );
        match case {
            "missing-mode" => {
                command.env_remove("APP_ENV");
            }
            "production" => {
                command.env("APP_ENV", "production");
            }
            "wrong-role" => {
                command.env(
                    "MEDIA_READ_DATABASE_URL",
                    "postgres://board_media@127.0.0.1/imageboard",
                );
            }
            "remote" => {
                command.env(
                    "MEDIA_READ_DATABASE_URL",
                    "postgres://board_media_read@database.example/imageboard",
                );
            }
            "writer" | "writer-inherits-reader" => {
                command.env(
                    "MEDIA_DATABASE_URL",
                    "postgres://board_media:synthetic@127.0.0.1:55432/imageboard",
                );
            }
            "public" | "public-inherits-reader" => {
                command.env(
                    "DATABASE_URL",
                    "postgres://board_public:synthetic@127.0.0.1:55432/imageboard",
                );
            }
            "owner" => {
                command.env("MIGRATION_DATABASE_URL", "synthetic-secret");
            }
            "staff" => {
                command.env("STAFF_DATABASE_URL", "synthetic-secret");
            }
            "auth" => {
                command.env("AUTH_DATABASE_URL", "synthetic-secret");
            }
            "test-public" => {
                command.env("TEST_PUBLIC_DATABASE_URL", "synthetic-secret");
            }
            "enabled" => {
                command.env("MEDIA_ENABLED", "true");
            }
            _ => (),
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "case {case}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn media_reader_config_child() {
    // Test child receives configuration solely from its parent; no environment
    // mutation is needed in a multithreaded Rust process.
    let Ok(case) = std::env::var("BOARD_READER_CONFIG_CASE") else {
        return;
    };
    if case == "valid" {
        assert!(MediaReaderSettings::from_env().is_ok());
    } else if case == "public-inherits-reader" {
        assert!(Settings::from_env().is_err());
    } else if case == "writer-inherits-reader" {
        assert!(MediaAdminSettings::from_env().is_err());
    } else {
        assert!(MediaReaderSettings::from_env().is_err());
    }
}
