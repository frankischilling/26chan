use board_media_dispatch::{
    ClientSettings, DispatchClient, config::GatewaySettings, protocol::read_request,
    tls::server_config,
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose,
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{PrivatePkcs8KeyDer, ServerName},
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

// Fresh keys each run; validity 2020..2040, except the explicitly expired 2020..2021 leaf.
// No generated key or certificate is a deployment asset.
struct Authority {
    certificate: Certificate,
    issuer: Issuer<'static, KeyPair>,
}
fn authority() -> Authority {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2040, 1, 1);
    let key = KeyPair::generate().unwrap();
    Authority {
        certificate: params.self_signed(&key).unwrap(),
        issuer: Issuer::new(params, key),
    }
}

fn leaf(ca: &Authority, server: bool, expired: bool) -> (Certificate, KeyPair) {
    let mut params = CertificateParams::new(vec!["dispatch.test".into()]).unwrap();
    params.extended_key_usages = vec![if server {
        ExtendedKeyUsagePurpose::ServerAuth
    } else {
        ExtendedKeyUsagePurpose::ClientAuth
    }];
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(if expired { 2021 } else { 2040 }, 1, 1);
    let key = KeyPair::generate().unwrap();
    (params.signed_by(&key, &ca.issuer).unwrap(), key)
}

fn private_file(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

struct Fixture {
    dir: TempDir,
    ca: Authority,
    #[cfg(target_os = "linux")]
    client: Certificate,
    #[cfg(target_os = "linux")]
    client_key: KeyPair,
    gateway: GatewaySettings,
    settings: ClientSettings,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = |name| dir.path().join(name);
        let ca = authority();
        let (server, server_key) = leaf(&ca, true, false);
        let (client, client_key) = leaf(&ca, false, false);
        private_file(&path("ca.pem"), ca.certificate.pem());
        private_file(&path("server.pem"), server.pem());
        private_file(&path("server.key"), server_key.serialize_pem());
        private_file(&path("client.pem"), client.pem());
        private_file(&path("client.key"), client_key.serialize_pem());
        private_file(
            &path("authorized"),
            format!("{:x}\n", Sha256::digest(client.der())),
        );
        let gateway = GatewaySettings {
            listen: "127.0.0.1:0".parse().unwrap(),
            server_certificate: path("server.pem"),
            server_key: path("server.key"),
            client_ca: path("ca.pem"),
            authorization_file: path("authorized"),
            broker_socket: path("broker.sock"),
        };
        let settings = ClientSettings {
            endpoint: gateway.listen,
            server_name: "dispatch.test".into(),
            server_ca: path("ca.pem"),
            client_certificate: path("client.pem"),
            client_key: path("client.key"),
        };
        Self {
            dir,
            ca,
            #[cfg(target_os = "linux")]
            client,
            #[cfg(target_os = "linux")]
            client_key,
            gateway,
            settings,
        }
    }

    async fn server(&mut self, response: Vec<u8>, close_notify: bool) -> JoinHandle<bool> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        self.settings.endpoint = listener.local_addr().unwrap();
        let acceptor = TlsAcceptor::from(server_config(&self.gateway).unwrap());
        tokio::spawn(async move {
            let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(4), async {
                let (socket, _) = listener.accept().await.unwrap();
                acceptor.accept(socket).await
            })
            .await
            else {
                return false;
            };
            if read_request(&mut stream).await.is_err() {
                return false;
            }
            if stream.write_all(&response).await.is_err() {
                return false;
            }
            if close_notify {
                let _ = stream.shutdown().await;
            } else {
                let _ = stream.flush().await;
            }
            true
        })
    }

    fn connector(&self, identity: Option<(&Certificate, &KeyPair)>) -> TlsConnector {
        let mut roots = RootCertStore::empty();
        roots.add(self.ca.certificate.der().clone()).unwrap();
        let builder =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_root_certificates(roots);
        let config = match identity {
            Some((cert, key)) => builder
                .with_client_auth_cert(
                    vec![cert.der().clone()],
                    PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
                )
                .unwrap(),
            None => builder.with_no_client_auth(),
        };
        TlsConnector::from(Arc::new(config))
    }
}

fn response(body: &[u8], declared: u64) -> Vec<u8> {
    [b"IBOUT001".as_slice(), &declared.to_be_bytes(), body].concat()
}

#[tokio::test]
async fn valid_mutual_tls_uses_write_close_then_reads_exact_disk() {
    let mut f = Fixture::new();
    let server = f
        .server(response(&vec![0x5a; 4_194_816], 4_194_816), true)
        .await;
    let output = DispatchClient::new(&f.settings)
        .unwrap()
        .process(b"abc".as_slice(), 3)
        .await
        .unwrap();
    assert_eq!(output, vec![0x5a; 4_194_816]);
    assert!(server.await.unwrap());
}

#[tokio::test]
async fn client_rejects_wrong_server_name_and_ca() {
    for wrong_ca in [false, true] {
        let mut f = Fixture::new();
        let server = f.server(Vec::new(), true).await;
        if wrong_ca {
            private_file(&f.settings.server_ca, authority().certificate.pem());
        } else {
            f.settings.server_name = "wrong.test".into();
        }
        assert!(
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(b"a".as_slice(), 1)
                .await
                .is_err()
        );
        assert!(!server.await.unwrap());
    }
}

#[tokio::test]
async fn server_rejects_absent_wrong_ca_expired_and_wrong_usage_client() {
    for case in 0..4 {
        let mut f = Fixture::new();
        let server = f.server(Vec::new(), true).await;
        let alien = authority();
        let (cert, key) = leaf(if case == 1 { &alien } else { &f.ca }, case == 3, case == 2);
        let connector = f.connector(if case == 0 { None } else { Some((&cert, &key)) });
        if let Ok(mut tls) = connector
            .connect(
                ServerName::try_from("dispatch.test").unwrap(),
                TcpStream::connect(f.settings.endpoint).await.unwrap(),
            )
            .await
        {
            let _ = tls.write_all(b"IBJOB001\0\0\0\0\0\0\0\x01a").await;
            let _ = tls.shutdown().await;
            assert!(tls.read(&mut [0; 1]).await.is_err());
        }
        assert!(!server.await.unwrap());
    }
}

#[tokio::test]
async fn client_rejects_response_truncation_trailing_bytes_and_missing_close_notify() {
    for (bytes, notify) in [
        (response(b"short", 4_194_816), true),
        (response(&vec![0; 4_194_817], 4_194_816), true),
        (response(&vec![0; 4_194_816], 4_194_816), false),
        (response(b"", 4_194_817), true),
    ] {
        let mut f = Fixture::new();
        let server = f.server(bytes, notify).await;
        assert!(
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(b"a".as_slice(), 1)
                .await
                .is_err()
        );
        let _ = server.await.unwrap();
    }
}

#[tokio::test]
async fn client_rejects_changed_source_before_clean_request_eof() {
    for length in [2, 4] {
        let mut f = Fixture::new();
        let server = f.server(Vec::new(), true).await;
        assert!(
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(b"abc".as_slice(), length)
                .await
                .is_err()
        );
        assert!(!server.await.unwrap());
    }
}

#[test]
fn config_rejects_unknown_fields_relative_paths_non_dns_name_and_unbounded_files() {
    let f = Fixture::new();
    let valid = serde_json::json!({"endpoint":"127.0.0.1:443", "server_name":"dispatch.test", "server_ca":f.settings.server_ca, "client_certificate":f.settings.client_certificate, "client_key":f.settings.client_key});
    let path = f.dir.path().join("client.json");
    private_file(&path, valid.to_string());
    assert!(ClientSettings::read(&path).is_ok());
    for (field, value) in [
        ("unexpected", "secret"),
        ("server_ca", "relative.pem"),
        ("server_name", "127.0.0.1"),
        ("server_name", "https://dispatch.test"),
        ("endpoint", "dispatch.test:443"),
    ] {
        let mut json = valid.clone();
        json[field] = value.into();
        private_file(&path, json.to_string());
        assert!(ClientSettings::read(&path).is_err());
    }
    private_file(&path, vec![b' '; 65_537]);
    assert!(ClientSettings::read(&path).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            &f.settings.client_key,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(DispatchClient::new(&f.settings).is_err());
    }
}

#[tokio::test]
async fn client_handshake_has_an_absolute_three_second_deadline() {
    let mut f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.settings.endpoint = listener.local_addr().unwrap();
    let client = DispatchClient::new(&f.settings).unwrap();
    let task = tokio::spawn(async move { client.process(b"a".as_slice(), 1).await });
    let (_socket, _) = listener.accept().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(4), task)
            .await
            .unwrap()
            .unwrap(),
        Err(board_media_dispatch::Error::Deadline)
    );
}

#[tokio::test]
async fn client_slow_source_cannot_extend_intake_deadline() {
    let mut f = Fixture::new();
    let server = f.server(Vec::new(), true).await;
    let (mut sender, receiver) = tokio::io::duplex(8);
    let slow = tokio::spawn(async move {
        for _ in 0..8 {
            if sender.write_all(b"a").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    });
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(4),
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(receiver, 8)
        )
        .await
        .unwrap(),
        Err(board_media_dispatch::Error::Deadline)
    );
    assert!(!server.await.unwrap());
    slow.abort();
}

#[tokio::test]
async fn client_processing_times_out_without_partial_output() {
    let mut f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.settings.endpoint = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(server_config(&f.gateway).unwrap());
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        assert_eq!(read_request(&mut stream).await.unwrap(), b"a");
        stream
            .write_all(&response(b"partial", 4_194_816))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(26),
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(b"a".as_slice(), 1)
        )
        .await
        .unwrap(),
        Err(board_media_dispatch::Error::Deadline)
    );
    server.abort();
}

#[cfg(target_os = "linux")]
mod root_gateway {
    use super::*;
    use board_media_dispatch::{
        gateway::Gateway,
        protocol::{write_request, write_response},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::{net::UnixListener, sync::Notify};

    fn enabled() -> bool {
        if std::env::var_os("MEDIA_DISPATCH_ROOT_TESTS").is_none() {
            return false;
        }
        assert_eq!(std::env::var("MEDIA_DISPATCH_ROOT_TESTS").unwrap(), "1");
        assert!(
            rustix::process::geteuid().is_root(),
            "explicit root integration needs root"
        );
        true
    }

    struct Running {
        gateway: JoinHandle<board_media_dispatch::Result<()>>,
        broker: JoinHandle<()>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
        calls: Arc<AtomicUsize>,
    }
    impl Drop for Running {
        fn drop(&mut self) {
            self.gateway.abort();
            self.broker.abort();
        }
    }

    async fn admitted_intake(f: &Fixture) -> tokio_rustls::client::TlsStream<TcpStream> {
        let connector = f.connector(Some((&f.client, &f.client_key)));
        let mut first = connector
            .connect(
                ServerName::try_from("dispatch.test").unwrap(),
                TcpStream::connect(f.settings.endpoint).await.unwrap(),
            )
            .await
            .unwrap();
        first
            .write_all(b"IBJOB001\0\0\0\0\0\0\0\x03a")
            .await
            .unwrap();
        first.flush().await.unwrap();
        let mut probe = connector
            .connect(
                ServerName::try_from("dispatch.test").unwrap(),
                TcpStream::connect(f.settings.endpoint).await.unwrap(),
            )
            .await
            .unwrap();
        // A busy second connection proves the first passed initial authorization and
        // acquired the work permit. Its request is still incomplete at this barrier.
        let result = tokio::time::timeout(Duration::from_secs(1), probe.read(&mut [0]))
            .await
            .unwrap();
        assert!(result.is_err() || result.unwrap() == 0);
        first
    }

    async fn start(f: &mut Fixture) -> Running {
        start_with_response(f, None).await
    }

    async fn start_with_response(f: &mut Fixture, wire: Option<Vec<u8>>) -> Running {
        let backend = UnixListener::bind(&f.gateway.broker_socket).unwrap();
        let listener = TcpListener::bind(f.gateway.listen).await.unwrap();
        f.settings.endpoint = listener.local_addr().unwrap();
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let (e, r, c) = (entered.clone(), release.clone(), calls.clone());
        let broker = tokio::spawn(async move {
            loop {
                let (mut stream, _) = backend.accept().await.unwrap();
                assert_eq!(read_request(&mut stream).await.unwrap(), b"abc");
                c.fetch_add(1, Ordering::SeqCst);
                e.notify_one();
                r.notified().await;
                if let Some(bytes) = &wire {
                    let _ = stream.write_all(bytes).await;
                    let _ = stream.shutdown().await;
                } else {
                    let _ = write_response(&vec![0x5a; 4_194_816], &mut stream).await;
                }
            }
        });
        let gateway = tokio::spawn(Gateway::new(&f.gateway).unwrap().serve(listener));
        Running {
            gateway,
            broker,
            entered,
            release,
            calls,
        }
    }

    #[tokio::test]
    async fn authorized_control_and_revocation_while_processing() {
        if !enabled() {
            return;
        }
        let mut f = Fixture::new();
        let running = start(&mut f).await;
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        running.release.notify_one();
        assert_eq!(control.await.unwrap().unwrap(), vec![0x5a; 4_194_816]);
        let client = DispatchClient::new(&f.settings).unwrap();
        let revoked = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        private_file(&f.gateway.authorization_file, "00".repeat(32));
        running.release.notify_one();
        assert!(revoked.await.unwrap().is_err());
        assert_eq!(running.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn revocation_after_handshake_before_intake_never_reaches_broker() {
        if !enabled() {
            return;
        }
        let mut f = Fixture::new();
        let running = start(&mut f).await;
        let mut stream = admitted_intake(&f).await;
        private_file(&f.gateway.authorization_file, "00".repeat(32));
        stream.write_all(b"bc").await.unwrap();
        stream.shutdown().await.unwrap();
        assert!(
            board_media_dispatch::protocol::read_response(&mut stream)
                .await
                .is_err()
        );
        assert_eq!(running.calls.load(Ordering::SeqCst), 0);
        private_file(
            &f.gateway.authorization_file,
            format!("{:x}\n", Sha256::digest(f.client.der())),
        );
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        running.release.notify_one();
        assert!(control.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn unavailable_or_malformed_authorization_after_handshake_rejects() {
        if !enabled() {
            return;
        }
        for value in [
            None,
            Some("".to_string()),
            Some("GG".repeat(32)),
            Some("00".repeat(32) + "\n\n"),
            Some(("00".repeat(32) + "\n").repeat(2)),
            Some((0..9).map(|i| format!("{i:064x}\n")).collect()),
        ] {
            let mut f = Fixture::new();
            let running = start(&mut f).await;
            let mut stream = admitted_intake(&f).await;
            if let Some(value) = value {
                private_file(&f.gateway.authorization_file, value);
            } else {
                std::fs::remove_file(&f.gateway.authorization_file).unwrap();
            }
            stream.write_all(b"bc").await.unwrap();
            stream.shutdown().await.unwrap();
            assert!(
                board_media_dispatch::protocol::read_response(&mut stream)
                    .await
                    .is_err()
            );
            assert_eq!(running.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn active_work_rejects_second_request_before_reading_input() {
        if !enabled() {
            return;
        }
        let mut f = Fixture::new();
        let running = start(&mut f).await;
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        let mut extra = f
            .connector(Some((&f.client, &f.client_key)))
            .connect(
                ServerName::try_from("dispatch.test").unwrap(),
                TcpStream::connect(f.settings.endpoint).await.unwrap(),
            )
            .await
            .unwrap();
        // No application input is sent. Admission must close promptly on its own.
        let read = tokio::time::timeout(Duration::from_secs(1), extra.read(&mut [0; 1])).await;
        assert!(read.is_ok());
        assert_eq!(running.calls.load(Ordering::SeqCst), 1);
        running.release.notify_one();
        assert!(control.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn four_handshakes_are_bounded_and_fifth_closes_immediately() {
        if !enabled() {
            return;
        }
        let mut f = Fixture::new();
        let running = start(&mut f).await;
        let mut sockets = Vec::new();
        for _ in 0..4 {
            sockets.push(TcpStream::connect(f.settings.endpoint).await.unwrap());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut fifth = TcpStream::connect(f.settings.endpoint).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), fifth.read(&mut [0]))
                .await
                .is_ok()
        );
        for mut socket in sockets {
            assert!(
                tokio::time::timeout(Duration::from_secs(4), socket.read(&mut [0]))
                    .await
                    .is_ok()
            );
        }
        assert_eq!(running.calls.load(Ordering::SeqCst), 0);
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        running.release.notify_one();
        assert!(control.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn gateway_intake_deadline_and_malformed_requests_do_not_reach_broker() {
        if !enabled() {
            return;
        }
        let mut f = Fixture::new();
        let running = start(&mut f).await;
        for data in [
            b"IBJOB001\0\0\0\0\0\0\0\x04abc".as_slice(),
            b"IBJOB001\0\0\0\0\0\0\0\x03abcd".as_slice(),
            b"IBJOB001\0\0\0\0\0\x80\0\x01".as_slice(),
        ] {
            let mut stream = f
                .connector(Some((&f.client, &f.client_key)))
                .connect(
                    ServerName::try_from("dispatch.test").unwrap(),
                    TcpStream::connect(f.settings.endpoint).await.unwrap(),
                )
                .await
                .unwrap();
            let _ = stream.write_all(data).await;
            let _ = stream.shutdown().await;
            assert!(
                board_media_dispatch::protocol::read_response(&mut stream)
                    .await
                    .is_err()
            );
        }
        let mut stream = f
            .connector(Some((&f.client, &f.client_key)))
            .connect(
                ServerName::try_from("dispatch.test").unwrap(),
                TcpStream::connect(f.settings.endpoint).await.unwrap(),
            )
            .await
            .unwrap();
        let start = tokio::time::Instant::now();
        for byte in b"IBJOB001" {
            if stream.write_all(&[*byte]).await.is_err() {
                break;
            }
            let _ = stream.flush().await;
            tokio::time::sleep(Duration::from_millis(700)).await;
            if start.elapsed() > Duration::from_millis(3200) {
                break;
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.read(&mut [0]))
                .await
                .is_ok()
        );
        assert_eq!(running.calls.load(Ordering::SeqCst), 0);
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        running.release.notify_one();
        assert!(control.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn writable_authorization_rejects_at_startup_and_before_response() {
        if !enabled() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut f = Fixture::new();
        std::fs::set_permissions(
            &f.gateway.authorization_file,
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        assert!(Gateway::new(&f.gateway).is_err());
        std::fs::set_permissions(
            &f.gateway.authorization_file,
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let running = start(&mut f).await;
        let client = DispatchClient::new(&f.settings).unwrap();
        let task = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        std::fs::set_permissions(
            &f.gateway.authorization_file,
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        running.release.notify_one();
        assert!(task.await.unwrap().is_err());
        assert_eq!(running.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_broker_responses_never_send_partial_tls_output() {
        if !enabled() {
            return;
        }
        for wire in [
            response(b"short", 4_194_816),
            response(&vec![0; 4_194_817], 4_194_816),
            response(b"", 4_194_817),
        ] {
            let mut f = Fixture::new();
            let running = start_with_response(&mut f, Some(wire)).await;
            let mut stream = f
                .connector(Some((&f.client, &f.client_key)))
                .connect(
                    ServerName::try_from("dispatch.test").unwrap(),
                    TcpStream::connect(f.settings.endpoint).await.unwrap(),
                )
                .await
                .unwrap();
            write_request(b"abc".as_slice(), 3, &mut stream)
                .await
                .unwrap();
            running.entered.notified().await;
            running.release.notify_one();
            let result = stream.read(&mut [0; 1]).await;
            assert!(result.is_err() || result.unwrap() == 0);
            assert_eq!(running.calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn gateway_exchange_has_an_absolute_deadline_and_cancellation_drops_connections() {
        if !enabled() {
            return;
        }
        for cancel in [false, true] {
            let mut f = Fixture::new();
            let running = start(&mut f).await;
            let mut stream = f
                .connector(Some((&f.client, &f.client_key)))
                .connect(
                    ServerName::try_from("dispatch.test").unwrap(),
                    TcpStream::connect(f.settings.endpoint).await.unwrap(),
                )
                .await
                .unwrap();
            write_request(b"abc".as_slice(), 3, &mut stream)
                .await
                .unwrap();
            running.entered.notified().await;
            if cancel {
                running.gateway.abort();
            }
            let deadline = if cancel {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(26)
            };
            let result = tokio::time::timeout(deadline, stream.read(&mut [0; 1]))
                .await
                .unwrap();
            assert!(result.is_err() || result.unwrap() == 0);
            assert_eq!(running.calls.load(Ordering::SeqCst), 1);
            if !cancel {
                // The transport deadline releases its slot; the backend has its own lifetime.
                running.release.notify_one();
                let client = DispatchClient::new(&f.settings).unwrap();
                let control =
                    tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
                running.entered.notified().await;
                running.release.notify_one();
                assert!(control.await.unwrap().is_ok());
            }
        }
    }

    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[tokio::test]
    async fn nonroot_broker_peer_receives_no_request_with_root_control() {
        if !enabled() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut f = Fixture::new();
        std::fs::set_permissions(f.dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let directory = f.dir.path().join("untrusted");
        std::fs::create_dir(&directory).unwrap();
        rustix::fs::chown(
            &directory,
            Some(rustix::process::Uid::from_raw(65534)),
            Some(rustix::process::Gid::from_raw(65534)),
        )
        .unwrap();
        f.gateway.broker_socket = directory.join("broker.sock");
        let marker = directory.join("received");
        let script = "import socket,sys,pathlib\ns=socket.socket(socket.AF_UNIX);s.bind(sys.argv[1]);s.listen(1)\nc,_=s.accept();c.settimeout(4);data=c.recv(1);pathlib.Path(sys.argv[2]).write_text(str(len(data)))";
        let mut child = Child(
            std::process::Command::new("/usr/sbin/runuser")
                .args(["-u", "nobody", "--", "/usr/bin/python3", "-c", script])
                .arg(&f.gateway.broker_socket)
                .arg(&marker)
                .env_clear()
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while !f.gateway.broker_socket.exists() {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "owned broker helper failed"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let listener = TcpListener::bind(f.gateway.listen).await.unwrap();
        f.settings.endpoint = listener.local_addr().unwrap();
        let gateway = tokio::spawn(Gateway::new(&f.gateway).unwrap().serve(listener));
        assert!(
            DispatchClient::new(&f.settings)
                .unwrap()
                .process(b"abc".as_slice(), 3)
                .await
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            while child.0.try_wait().unwrap().is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "0");
        gateway.abort();
        std::fs::remove_file(&f.gateway.broker_socket).unwrap();
        let running = start(&mut f).await;
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        running.entered.notified().await;
        running.release.notify_one();
        assert!(control.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn production_cli_rejects_root_and_unrelated_environment_without_echoing_values() {
        if !enabled() {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let mut f = Fixture::new();
        std::fs::set_permissions(f.dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        for path in [
            &f.gateway.server_certificate,
            &f.gateway.client_ca,
            &f.gateway.authorization_file,
        ] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        f.gateway.listen = reservation.local_addr().unwrap();
        f.settings.endpoint = f.gateway.listen;
        let config = f.dir.path().join("gateway.json");
        private_file(&config, serde_json::json!({"listen":f.gateway.listen, "server_certificate":f.gateway.server_certificate, "server_key":f.gateway.server_key, "client_ca":f.gateway.client_ca, "authorization_file":f.gateway.authorization_file, "broker_socket":f.gateway.broker_socket}).to_string());
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
        let backend = UnixListener::bind(&f.gateway.broker_socket).unwrap();
        std::fs::set_permissions(
            &f.gateway.broker_socket,
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        let binary = env!("CARGO_BIN_EXE_media-dispatch-gateway");
        let root = std::process::Command::new(binary)
            .env_clear()
            .env("APP_ENV", "development")
            .arg(&config)
            .output()
            .unwrap();
        assert!(!root.status.success());
        assert!(root.stdout.is_empty());
        assert_eq!(root.stderr, b"invalid configuration\n");
        rustix::fs::chown(
            &f.gateway.server_key,
            Some(rustix::process::Uid::from_raw(65534)),
            Some(rustix::process::Gid::from_raw(65534)),
        )
        .unwrap();
        let output = std::process::Command::new("/usr/bin/setpriv")
            .args([
                "--reuid=65534",
                "--regid=65534",
                "--clear-groups",
                "/usr/bin/env",
                "-i",
                "APP_ENV=development",
                "DATABASE_URL=synthetic-do-not-print",
                binary,
            ])
            .arg(&config)
            .env_clear()
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"invalid configuration\n");
        drop(reservation);
        let mut child = Child(
            std::process::Command::new("/usr/bin/setpriv")
                .args([
                    "--reuid=65534",
                    "--regid=65534",
                    "--clear-groups",
                    "/usr/bin/env",
                    "-i",
                    "APP_ENV=development",
                    binary,
                ])
                .arg(&config)
                .env_clear()
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "nonroot gateway control failed to start"
                );
                if TcpStream::connect(f.settings.endpoint).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let client = DispatchClient::new(&f.settings).unwrap();
        let control = tokio::spawn(async move { client.process(b"abc".as_slice(), 3).await });
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(4), backend.accept())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(socket.peer_cred().unwrap().uid(), 65534);
        assert_eq!(read_request(&mut socket).await.unwrap(), b"abc");
        write_response(&vec![0x5a; 4_194_816], &mut socket)
            .await
            .unwrap();
        assert_eq!(control.await.unwrap().unwrap(), vec![0x5a; 4_194_816]);
    }
}
