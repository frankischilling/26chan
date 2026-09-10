use board_media::{ApprovedFiles, PublicationStore, Quarantine};

#[test]
fn metrics_startup_failure_never_connects_the_reader_or_leaves_media_serving() {
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let _store = PublicationStore::new(&root, &quarantine).unwrap();
    ApprovedFiles::open(&root).unwrap().ready().unwrap();
    let database = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    database.set_nonblocking(true).unwrap();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    for token in ["synthetic-secret".to_owned(), "a".repeat(64)] {
        let media = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let media_address = media.local_addr().unwrap();
        drop(media);
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_board-media-http"));
        command.env_clear();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        let output = command
            .env("APP_ENV", "development")
            .env("MEDIA_ORIGIN", format!("http://{media_address}"))
            .env("MEDIA_BIND_ADDR", media_address.to_string())
            .env("MEDIA_APPROVED_DIR", &root)
            .env(
                "MEDIA_READ_DATABASE_URL",
                format!(
                    "postgres://board_media_read:unused@{}/absent",
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
        let logs = String::from_utf8_lossy(&output.stdout);
        assert!(
            !error.contains("configuration rejected"),
            "base configuration must be valid"
        );
        assert!(!error.contains(&token));
        assert!(!logs.contains(&token));
        assert!(!error.contains("unused"));
        assert!(!logs.contains("unused"));
        assert!(std::net::TcpListener::bind(media_address).is_ok());
        assert_eq!(
            database.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
