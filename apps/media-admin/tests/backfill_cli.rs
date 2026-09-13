use std::process::Command;

#[test]
fn legacy_upgrade_refuses_runtime_credentials_and_unsafe_profiles_before_storage() {
    for (key, value) in [
        ("APP_ENV", "production"),
        ("APP_ENV", ""),
        ("DATABASE_URL", "synthetic-secret"),
        ("AUTH_DATABASE_URL", "synthetic-secret"),
        ("STAFF_DATABASE_URL", "synthetic-secret"),
        ("MEDIA_DATABASE_URL", "synthetic-secret"),
        ("MEDIA_READ_DATABASE_URL", "synthetic-secret"),
        ("INTAKE_DATABASE_URL", "synthetic-secret"),
        ("TEST_PUBLIC_DATABASE_URL", "synthetic-secret"),
        ("MONITOR_DATABASE_URL", "synthetic-secret"),
        ("PUBLIC_INTAKE_TOKEN", "synthetic-secret"),
        ("MEDIA_GROUP_READ", "yes"),
        (
            "MIGRATION_DATABASE_URL",
            "postgres://board_public:synthetic-secret@127.0.0.1/db",
        ),
        (
            "MIGRATION_DATABASE_URL",
            "postgres://board_migrator:synthetic-secret@remote.example/db",
        ),
        (
            "MIGRATION_DATABASE_URL",
            "postgres://board_migrator:synthetic-secret@127.0.0.1/db?user=postgres",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("must-not-create");
        let output = Command::new(env!("CARGO_BIN_EXE_media-backfill"))
            .env_clear()
            .env("APP_ENV", "development")
            .env(
                "MIGRATION_DATABASE_URL",
                "postgres://board_migrator:synthetic-secret@127.0.0.1:1/absent",
            )
            .env(key, value)
            .arg("absent-config")
            .arg(&missing)
            .arg(&missing)
            .arg("a".repeat(32))
            .output()
            .unwrap();
        assert!(!output.status.success(), "{key}");
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-secret"));
        assert!(!missing.exists());
    }
}
