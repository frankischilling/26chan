use std::{
    net::TcpListener,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn output(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("observer startup did not reject within five seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

fn command(database: &TcpListener, metrics: &TcpListener) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-monitor"));
    command
        .env_clear()
        .env("APP_ENV", "development")
        .env(
            "MONITOR_DATABASE_URL",
            format!(
                "postgres://board_monitor:synthetic-secret@{}/absent",
                database.local_addr().unwrap()
            ),
        )
        .env(
            "METRICS_BIND_ADDR",
            metrics.local_addr().unwrap().to_string(),
        )
        .env("METRICS_TOKEN", "a".repeat(64));
    if let Some(value) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", value);
    }
    command
}

#[test]
fn startup_rejects_invalid_settings_credentials_and_occupied_exporter_before_connecting() {
    let database = TcpListener::bind("127.0.0.1:0").unwrap();
    database.set_nonblocking(true).unwrap();
    let metrics = TcpListener::bind("127.0.0.1:0").unwrap();
    for case in [
        "occupied",
        "missing-mode",
        "missing-metrics",
        "invalid-token",
        "wrong-role",
        "DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "PGOPTIONS",
        "PGPASSWORD",
        "PGPORT",
    ] {
        let mut cmd = command(&database, &metrics);
        match case {
            "occupied" => (),
            "missing-mode" => {
                cmd.env_remove("APP_ENV");
            }
            "missing-metrics" => {
                cmd.env_remove("METRICS_BIND_ADDR")
                    .env_remove("METRICS_TOKEN");
            }
            "invalid-token" => {
                cmd.env("METRICS_TOKEN", "synthetic-token");
            }
            "wrong-role" => {
                cmd.env(
                    "MONITOR_DATABASE_URL",
                    "postgres://board_public:synthetic-secret@127.0.0.1/absent",
                );
            }
            name => {
                cmd.env(name, "synthetic-other-secret");
            }
        }
        let output = output(cmd);
        assert!(!output.status.success(), "{case}");
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !logs.contains("synthetic-") && !logs.contains(&"a".repeat(64)),
            "{case}"
        );
        if case == "occupied" {
            assert!(logs.contains("stopped with an error"), "{case}: {logs}");
        } else {
            assert!(logs.contains("configuration rejected"), "{case}: {logs}");
        }
        assert_eq!(
            database.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "{case}"
        );
    }
}
