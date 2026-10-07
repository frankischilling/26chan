//! Controlled real TLS responses isolate coordinator fencing. The native harness
//! separately qualifies the real gateway, root broker and Firecracker decoder.
use board_media_dispatch::{config::GatewaySettings, protocol::read_request, tls::server_config};
use board_store::media::MediaQueue;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::oneshot};
use tokio_rustls::TlsAcceptor;

#[derive(Debug, Default, PartialEq, Eq, sqlx::FromRow)]
struct SourceProvenanceRow {
    source_input_sha256: Option<String>,
    source_input_bytes: Option<i64>,
    source_profile: Option<String>,
    source_retained_bytes: Option<i64>,
    source_md5: Option<Vec<u8>>,
}

fn private(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

// Independent valid 1x1 RGBA PNG, a dropped tEXt chunk, and trailer.
// Golden digests are from Python hashlib over explicit fixture bytes.
fn source_png() -> Vec<u8> {
    let red = hex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c48900000010494441547801010500faff00ff0000ff050001fffa5c88d10000000049454e44ae426082",
    );
    [
        red[..33].to_vec(),
        hex("00000003744558746e0076cdcf317b"),
        red[33..].to_vec(),
        b"ignored trailer".to_vec(),
    ]
    .concat()
}

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
        .collect()
}

pub async fn exercise(queue: &MediaQueue, admin: &sqlx::PgPool, ids: &Mutex<Vec<String>>) {
    let temp = tempfile::tempdir().unwrap();
    let path = |name: &str| temp.path().join(name);
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    private(&path("ca.pem"), ca.self_signed(&ca_key).unwrap().pem());
    let issuer = Issuer::new(ca, ca_key);
    for (name, usage) in [
        ("server", ExtendedKeyUsagePurpose::ServerAuth),
        ("client", ExtendedKeyUsagePurpose::ClientAuth),
    ] {
        let mut params = CertificateParams::new(vec!["dispatch.test".into()]).unwrap();
        params.extended_key_usages = vec![usage];
        let key = KeyPair::generate().unwrap();
        private(
            &path(&format!("{name}.pem")),
            params.signed_by(&key, &issuer).unwrap().pem(),
        );
        private(&path(&format!("{name}.key")), key.serialize_pem());
    }
    let quarantine = board_media::Quarantine::new(path("quarantine")).unwrap();
    for case in [
        "valid",
        "snapshot-mutation",
        "jpeg",
        "gif",
        "malformed-png",
        "truncated-png",
        "excess-chunk",
        "unknown-format",
        "production",
        "DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "MONITOR_DATABASE_URL",
        "configuration",
        "roots",
        "transport",
        "invalid",
        "decoder-rejection",
        "expired",
        "replaced",
        "changed-input",
    ] {
        let environment_denial = match case {
            "production" => Some(("APP_ENV", "production")),
            name if name.ends_with("DATABASE_URL") => {
                Some((name, "synthetic-secret-must-not-appear"))
            }
            _ => None,
        };
        let listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let endpoint = listener.local_addr().unwrap();
        let gateway = GatewaySettings {
            listen: endpoint,
            server_certificate: path("server.pem"),
            server_key: path("server.key"),
            client_ca: path("ca.pem"),
            authorization_file: path("unused"),
            broker_socket: path("unused.sock"),
        };
        private(&path("client.json"), serde_json::to_vec(&serde_json::json!({
            "endpoint": endpoint.to_string(), "server_name": "dispatch.test", "server_ca": path("ca.pem"),
            "client_certificate": path("client.pem"), "client_key": path("client.key")
        })).unwrap());
        let job = queue
            .reserve("private-filename-must-not-cross-transport.png")
            .await
            .unwrap();
        ids.lock().unwrap().push(job.id.clone());
        let input = match case {
            "jpeg" => b"\xff\xd8synthetic decoder fixture".to_vec(),
            "gif" => b"GIF89asynthetic decoder fixture".to_vec(),
            "malformed-png" => b"\x89PNG\r\n\x1a\n".to_vec(),
            "truncated-png" => source_png()[..43].to_vec(),
            "excess-chunk" => b"\x89PNG\r\n\x1a\n\xff\xff\xff\xffIDAT".to_vec(),
            "unknown-format" => b"unknown".to_vec(),
            "decoder-rejection" => {
                // Framing is scanner-compatible but not decoder-admissible:
                // corrupt IHDR CRC, then simulate guest output rejection.
                let mut bytes = source_png();
                bytes[29] ^= 1;
                bytes
            }
            _ => source_png(),
        };
        let length = quarantine
            .receive(job.id.parse().unwrap(), input.as_slice())
            .await
            .unwrap();
        queue.queue(&job.id, length).await.unwrap();
        if case == "configuration" {
            private(&path("client.json"), b"{}");
        }
        if case == "changed-input" {
            std::fs::write(
                path("quarantine").join(format!("{}.input", job.id)),
                b"changed",
            )
            .unwrap();
        }
        let (received, arrival) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let acceptor = TlsAcceptor::from(server_config(&gateway).unwrap());
        let server_listener = listener.clone();
        let accepted = Arc::new(AtomicBool::new(false));
        let server_accepted = accepted.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = server_listener.accept().await.unwrap();
            server_accepted.store(true, Ordering::SeqCst);
            let mut stream = acceptor.accept(socket).await.unwrap();
            let received_input = read_request(&mut stream).await.unwrap();
            assert_eq!(received_input, input);
            received.send(()).unwrap();
            // The environment cases must have a working full response if a
            // guard regresses, rather than fail on a test barrier or timeout.
            if environment_denial.is_none() {
                released.await.unwrap();
            }
            if case == "transport" {
                return;
            }
            let mut disk = vec![0; 4_194_816];
            if !["invalid", "decoder-rejection"].contains(&case) {
                disk[..20].copy_from_slice(b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff");
            }
            stream.write_all(b"IBOUT001").await.unwrap();
            stream
                .write_all(&4_194_816_u64.to_be_bytes())
                .await
                .unwrap();
            stream.write_all(&disk).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let mut cmd = super::command(env!("CARGO_BIN_EXE_media-publish"), "MEDIA_DATABASE_URL");
        cmd.env("MEDIA_QUARANTINE_DIR", path("quarantine"))
            .arg("dispatch")
            .arg(path("client.json"))
            .arg(if case == "roots" {
                path("quarantine")
            } else {
                path("objects")
            });
        if let Some((name, value)) = environment_denial {
            cmd.env(name, value);
        }
        let task = tokio::task::spawn_blocking(move || cmd.output().unwrap());
        if environment_denial.is_some() {
            let result = task.await.unwrap();
            server.abort();
            let _ = server.await;
            assert!(!result.status.success(), "case {case}");
            assert!(result.stdout.is_empty(), "case {case}");
            assert_eq!(
                String::from_utf8(result.stderr).unwrap().trim(),
                "media publication command rejected; inspect private state before retrying",
                "case {case}"
            );
            let unchanged = queue.get(&job.id).await.unwrap();
            assert_eq!(unchanged.state, "queued", "case {case}");
            assert_eq!(unchanged.attempts, 0, "case {case}");
            assert!(unchanged.lease_token.is_none(), "case {case}");
            assert!(
                !accepted.load(Ordering::SeqCst),
                "case {case} reached transport"
            );
            // The child has exited and the accept task has joined. Check the
            // kernel backlog too, so scheduling cannot mask a connection.
            let listener = Arc::try_unwrap(listener).unwrap().into_std().unwrap();
            assert!(
                matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                "case {case} left a pending transport connection"
            );
        } else if [
            "configuration",
            "roots",
            "changed-input",
            "malformed-png",
            "truncated-png",
            "excess-chunk",
            "unknown-format",
        ]
        .contains(&case)
        {
            let result = task.await.unwrap();
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            assert_eq!(
                queue.get(&job.id).await.unwrap().state,
                if !["configuration", "roots"].contains(&case) {
                    "failed"
                } else {
                    "queued"
                }
            );
            if !["configuration", "roots"].contains(&case) {
                assert_eq!(
                    queue.get(&job.id).await.unwrap().failure.as_deref(),
                    Some("processing_failed"),
                    "case {case} must retain the durable processing failure class"
                );
            }
            server.abort();
            let _ = server.await;
            assert!(
                !accepted.load(Ordering::SeqCst),
                "case {case} reached transport"
            );
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=$1")
                    .bind(&job.id)
                    .fetch_one(admin)
                    .await
                    .unwrap();
            assert_eq!(count, 0, "case {case}");
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(5), arrival)
                .await
                .expect("dispatch command must reach authenticated transport")
                .unwrap();
            if case == "snapshot-mutation" {
                let input_path = path("quarantine").join(format!("{}.input", job.id));
                std::fs::write(&input_path, b"mutation after snapshot").unwrap();
                std::fs::rename(&input_path, path("replaced-input")).unwrap();
                std::fs::write(&input_path, b"new pathname contents").unwrap();
            }
            let expired_at = if case == "expired" {
                Some(super::expire(admin, &job.id).await)
            } else {
                None
            };
            if case == "replaced" {
                sqlx::query("UPDATE media.jobs SET lease_token=replace(gen_random_uuid()::text,'-','') WHERE id=$1")
                    .bind(&job.id).execute(admin).await.unwrap();
            }
            release.send(()).unwrap();
            let result = task.await.unwrap();
            server.await.unwrap();
            let observed_at: sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc> =
                sqlx::query_scalar("SELECT clock_timestamp()")
                    .fetch_one(admin)
                    .await
                    .unwrap();
            assert_eq!(
                result.status.success(),
                ["valid", "snapshot-mutation", "jpeg", "gif"].contains(&case),
                "case {case}: {}; expired_at={expired_at:?}; observed_database_clock={observed_at}",
                String::from_utf8_lossy(&result.stderr)
            );
            if ["valid", "snapshot-mutation", "jpeg", "gif"].contains(&case) {
                let id = String::from_utf8(result.stdout).unwrap();
                let id = id.trim();
                assert!(id.parse::<board_media::ObjectId>().is_ok());
                assert_ne!(id, job.id);
                let source: SourceProvenanceRow =
                    sqlx::query_as("SELECT source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5 FROM media.assets WHERE id=$1")
                        .bind(id).fetch_one(admin).await.unwrap();
                if ["jpeg", "gif"].contains(&case) {
                    assert_eq!(source, SourceProvenanceRow::default());
                } else {
                    assert_eq!(
                        source,
                        SourceProvenanceRow {
                            source_input_sha256: Some(
                                "f482035298dcdf31dca1ce576adea047626ddb9deff6c5bfce3cf1ea68b15357"
                                    .into()
                            ),
                            source_input_bytes: Some(103),
                            source_profile: Some("png-v1".into()),
                            source_retained_bytes: Some(73),
                            source_md5: Some(hex("b4e7464f29bcc44451c570504d61030b")),
                        }
                    );
                }
                let reader = board_store::media_assets::MediaReader::connect(
                    &std::env::var("MEDIA_READ_DATABASE_URL").unwrap(),
                )
                .await
                .unwrap();
                let files = board_media::ApprovedFiles::open(path("objects")).unwrap();
                assert!(
                    board_media_admin::read_approved(&reader, &files, id)
                        .await
                        .unwrap()
                        .starts_with(b"\x89PNG")
                );
                assert!(
                    board_media_admin::read_approved(&reader, &files, &job.id)
                        .await
                        .is_err()
                );
            } else {
                assert!(result.stdout.is_empty());
                let count: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=$1")
                        .bind(&job.id)
                        .fetch_one(admin)
                        .await
                        .unwrap();
                assert_eq!(count, 0, "case {case}");
                let job = queue.get(&job.id).await.unwrap();
                match case {
                    "invalid" | "decoder-rejection" => {
                        assert_eq!(job.failure.as_deref(), Some("invalid_output"))
                    }
                    "transport" => assert_eq!(job.failure.as_deref(), Some("processing_failed")),
                    _ => {
                        assert_eq!(job.state, "processing");
                        assert!(job.failure.is_none());
                    }
                }
            }
        }
        // Retire only this owned record; never expire another test's queue.
        sqlx::query("UPDATE media.jobs SET state='failed',lease_token=NULL,expires_at=NULL,failure='abandoned' WHERE id=$1 AND state <> 'published'")
            .bind(&job.id).execute(admin).await.unwrap();
    }
}
