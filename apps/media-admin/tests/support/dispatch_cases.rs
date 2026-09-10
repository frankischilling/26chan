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

fn private(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
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
        "production",
        "DATABASE_URL",
        "MIGRATION_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "configuration",
        "roots",
        "transport",
        "invalid",
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
        quarantine
            .receive(job.id.parse().unwrap(), b"exact-input".as_slice())
            .await
            .unwrap();
        queue.queue(&job.id, 11).await.unwrap();
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
            let input = read_request(&mut stream).await.unwrap();
            assert_eq!(input, b"exact-input");
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
            if case != "invalid" {
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
        } else if ["configuration", "roots", "changed-input"].contains(&case) {
            let result = task.await.unwrap();
            assert!(!result.status.success());
            assert!(result.stdout.is_empty());
            assert_eq!(
                queue.get(&job.id).await.unwrap().state,
                if case == "changed-input" {
                    "failed"
                } else {
                    "queued"
                }
            );
            server.abort();
            let _ = server.await;
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(5), arrival)
                .await
                .expect("dispatch command must reach authenticated transport")
                .unwrap();
            if case == "expired" {
                super::expire(admin, &job.id).await;
            }
            if case == "replaced" {
                sqlx::query("UPDATE media.jobs SET lease_token=replace(gen_random_uuid()::text,'-','') WHERE id=$1")
                    .bind(&job.id).execute(admin).await.unwrap();
            }
            release.send(()).unwrap();
            let result = task.await.unwrap();
            server.await.unwrap();
            assert_eq!(
                result.status.success(),
                case == "valid",
                "case {case}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            if case == "valid" {
                let id = String::from_utf8(result.stdout).unwrap();
                let id = id.trim();
                assert!(id.parse::<board_media::ObjectId>().is_ok());
                assert_ne!(id, job.id);
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
                    "invalid" => assert_eq!(job.failure.as_deref(), Some("invalid_output")),
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
