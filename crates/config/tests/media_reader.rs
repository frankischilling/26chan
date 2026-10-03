use board_config::{MediaAdminSettings, MediaReaderSettings, MonitorSettings, Settings};
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
        "staff-trip-key",
        "public-staff-trip-key",
        "writer-staff-trip-key",
        "public-identity-clean",
        "writer-identity-clean",
        "observer-staff-trip-key",
        "observer-identity-clean",
        "test-public",
        "enabled",
        "public-inherits-reader",
        "writer-inherits-reader",
        "query-host",
        "query-hostaddr",
        "query-user",
        "query-port",
        "query-option",
        "fragment",
        "writer-query",
        "writer-fragment",
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
            "staff-trip-key"
            | "public-staff-trip-key"
            | "writer-staff-trip-key"
            | "public-identity-clean"
            | "writer-identity-clean" => {
                if case.ends_with("trip-key") {
                    command.env("STAFF_TRIPCODE_KEY", "11".repeat(32));
                }
                if case != "staff-trip-key" {
                    command.env_remove("MEDIA_READ_DATABASE_URL");
                }
                if case.starts_with("public-") {
                    command.env(
                        "DATABASE_URL",
                        "postgres://board_public:synthetic@127.0.0.1:55432/imageboard",
                    );
                } else if case.starts_with("writer-") {
                    command.env(
                        "MEDIA_DATABASE_URL",
                        "postgres://board_media:synthetic@127.0.0.1:55432/imageboard",
                    );
                }
            }
            "observer-staff-trip-key" | "observer-identity-clean" => {
                command.env_remove("MEDIA_READ_DATABASE_URL");
                command.env(
                    "MONITOR_DATABASE_URL",
                    "postgres://board_monitor:synthetic@127.0.0.1:55432/imageboard",
                );
                if case.ends_with("trip-key") {
                    command.env("STAFF_TRIPCODE_KEY", "11".repeat(32));
                }
            }
            "test-public" => {
                command.env("TEST_PUBLIC_DATABASE_URL", "synthetic-secret");
            }
            "enabled" => {
                command.env("MEDIA_ENABLED", "true");
            }
            "writer-query" | "writer-fragment" => {
                command.env_remove("MEDIA_READ_DATABASE_URL");
                let suffix = if case == "writer-query" {
                    "?application_name=fixture"
                } else {
                    "#fixture"
                };
                command.env(
                    "MEDIA_DATABASE_URL",
                    format!("postgres://board_media:synthetic@127.0.0.1:55432/imageboard{suffix}"),
                );
            }
            "query-host" | "query-hostaddr" | "query-user" | "query-port" | "query-option"
            | "fragment" => {
                let suffix = match case {
                    "query-host" => "?host=127.0.0.1",
                    "query-hostaddr" => "?hostaddr=127.0.0.1",
                    "query-user" => "?user=board_media_read",
                    "query-port" => "?port=55432",
                    "query-option" => "?application_name=fixture",
                    _ => "#fixture",
                };
                command.env(
                    "MEDIA_READ_DATABASE_URL",
                    format!(
                        "postgres://board_media_read:synthetic@127.0.0.1:55432/imageboard{suffix}"
                    ),
                );
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
    } else if case == "public-identity-clean" {
        assert!(Settings::from_env().is_ok());
    } else if case == "writer-identity-clean" {
        assert!(MediaAdminSettings::from_env().is_ok());
    } else if case == "observer-identity-clean" {
        assert!(MonitorSettings::from_env().is_ok());
    } else if case == "observer-staff-trip-key" {
        assert!(MonitorSettings::from_env().is_err());
    } else if ["public-inherits-reader", "public-staff-trip-key"].contains(&case.as_str()) {
        assert!(Settings::from_env().is_err());
    } else if [
        "writer-inherits-reader",
        "writer-query",
        "writer-fragment",
        "writer-staff-trip-key",
    ]
    .contains(&case.as_str())
    {
        assert!(MediaAdminSettings::from_env().is_err());
    } else {
        assert!(MediaReaderSettings::from_env().is_err());
    }
}
