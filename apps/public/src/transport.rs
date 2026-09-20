//! Public transport owns connections, their deadlines and kernel peer identity.
use axum::extract::ConnectInfo;
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

/// Clone this budget to share admission between public and JSON API listeners.
#[derive(Clone)]
pub struct ConnectionBudget {
    admission: Arc<Semaphore>,
    header_timeout: Duration,
    connection_timeout: Duration,
}

impl ConnectionBudget {
    pub fn new(limits: board_config::PublicRequestLimits) -> Self {
        Self {
            admission: Arc::new(Semaphore::new(limits.connections())),
            header_timeout: limits.header_timeout(),
            connection_timeout: limits.connection_timeout(),
        }
    }
}

pub enum PublicListener {
    Tcp(tokio::net::TcpListener),
    #[cfg(target_os = "linux")]
    Unix(UnixListener),
}

impl PublicListener {
    pub async fn bind(
        address: SocketAddr,
        proxy: Option<&board_config::PublicProxy>,
    ) -> io::Result<Self> {
        if let Some(proxy) = proxy {
            #[cfg(target_os = "linux")]
            {
                return UnixListener::bind(proxy.socket()).map(Self::Unix);
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = proxy;
                return Err(io::Error::other(
                    "Verified public proxy sockets require Linux",
                ));
            }
        }
        tokio::net::TcpListener::bind(address).await.map(Self::Tcp)
    }

    pub async fn serve(
        self,
        app: axum::Router,
        stopped: watch::Receiver<bool>,
        budget: ConnectionBudget,
    ) -> io::Result<()> {
        // Retain every task so cancelling this server aborts its connections.
        // Separate tasks preserve connection scheduling and panic isolation.
        let mut connections = tokio::task::JoinSet::new();
        let mut next_accept = tokio::time::Instant::now();
        loop {
            tokio::select! {
                biased;
                () = wait_for_stop(stopped.clone()) => break,
                result = connections.join_next(), if !connections.is_empty() => {
                    if result.is_some_and(|result| result.is_err()) {
                        tracing::warn!("public connection task terminated");
                    }
                },
                accepted = async {
                    tokio::time::sleep_until(next_accept).await;
                    self.accept().await
                } => {
                    let accepted = match accepted {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            // Back off without suspending existing connection
                            // futures, their deadlines or the shutdown signal.
                            tracing::warn!(kind = ?error.kind(), "public listener accept failed");
                            next_accept = tokio::time::Instant::now() + Duration::from_secs(1);
                            continue;
                        }
                    };
                    let deadline = tokio::time::Instant::now() + budget.connection_timeout;
                    // Acquire after accept so an idle listener cannot reserve
                    // capacity that belongs to the other listener. Excess
                    // sockets close without parsing a request or queuing work.
                    let Ok(permit) = budget.admission.clone().try_acquire_owned() else {
                        continue;
                    };
                    match accepted {
                        Accepted::Tcp(stream, peer) => {
                            connections.spawn(serve_connection(
                                stream, peer, app.clone(), stopped.clone(), permit,
                                budget.header_timeout, deadline,
                            ));
                        }
                        #[cfg(target_os = "linux")]
                        Accepted::Unix(stream) => {
                            let peer = crate::proxy_peer::UnixPeer(
                                stream.peer_cred().ok().map(|credentials| credentials.uid()),
                            );
                            connections.spawn(serve_connection(
                                stream, peer, app.clone(), stopped.clone(), permit,
                                budget.header_timeout, deadline,
                            ));
                        }
                    }
                }
            }
        }
        // Each connection observes the same stop signal and drains under its
        // original deadline. The Unix socket guard remains owned until then.
        while connections.join_next().await.is_some() {}
        Ok(())
    }

    async fn accept(&self) -> io::Result<Accepted> {
        match self {
            Self::Tcp(listener) => listener
                .accept()
                .await
                .map(|(stream, peer)| Accepted::Tcp(stream, peer)),
            #[cfg(target_os = "linux")]
            Self::Unix(listener) => listener
                .listener
                .accept()
                .await
                .map(|(stream, _)| Accepted::Unix(stream)),
        }
    }
}

enum Accepted {
    Tcp(tokio::net::TcpStream, SocketAddr),
    #[cfg(target_os = "linux")]
    Unix(tokio::net::UnixStream),
}

async fn serve_connection<I, P>(
    stream: I,
    peer: P,
    app: axum::Router,
    stopped: watch::Receiver<bool>,
    permit: OwnedSemaphorePermit,
    header_timeout: Duration,
    deadline: tokio::time::Instant,
) where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    P: Clone + Send + Sync + 'static,
{
    let _permit = permit;
    let router = hyper_util::service::TowerToHyperService::new(app);
    let service =
        hyper::service::service_fn(move |mut request: hyper::Request<hyper::body::Incoming>| {
            // Set the actual listener identity on every request, including reuse
            // of the connection. Headers never supply this extension.
            request.extensions_mut().insert(ConnectInfo(peer.clone()));
            hyper::service::Service::call(&router, request)
        });
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(header_timeout);
    let connection = builder.serve_connection(hyper_util::rt::TokioIo::new(stream), service);
    tokio::pin!(connection);
    let exchange = async {
        tokio::select! {
            biased;
            () = wait_for_stop(stopped) => {
                connection.as_mut().graceful_shutdown();
                let _ = connection.await;
            }
            _ = &mut connection => {}
        }
    };
    // This deadline starts at acceptance and includes silent connections,
    // headers, request bodies, handlers, response writes and keep-alive reuse.
    // Client errors may contain private input, so do not log their contents.
    let _ = tokio::time::timeout_at(deadline, exchange).await;
}

async fn wait_for_stop(mut stopped: watch::Receiver<bool>) {
    if !*stopped.borrow_and_update() {
        let _ = stopped.changed().await;
    }
}

#[cfg(target_os = "linux")]
pub struct UnixListener {
    listener: tokio::net::UnixListener,
    _guard: SocketGuard,
}

#[cfg(target_os = "linux")]
impl UnixListener {
    fn bind(path: &std::path::Path) -> io::Result<Self> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Socket needs a parent directory"))?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || std::fs::canonicalize(parent)? != parent
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o022 != 0
        {
            return Err(io::Error::other(
                "Socket parent must be canonical, owned by this process user and not writable by other users",
            ));
        }
        // Bind refuses an existing file, socket or symlink. Never unlink an
        // unverified pre-existing path to make startup succeed.
        let listener = tokio::net::UnixListener::bind(path)?;
        let metadata = std::fs::symlink_metadata(path)?;
        let guard = SocketGuard {
            path: path.to_owned(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
        Ok(Self {
            listener,
            _guard: guard,
        })
    }
}

#[cfg(target_os = "linux")]
struct SocketGuard {
    path: std::path::PathBuf,
    dev: u64,
    ino: u64,
}

#[cfg(target_os = "linux")]
impl Drop for SocketGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[tokio::test]
    async fn unix_and_tcp_connections_share_admission_before_headers_are_complete() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("public.sock");
        let public = PublicListener::Unix(UnixListener::bind(&path).unwrap());
        let api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = api.local_addr().unwrap();
        let limits = board_config::PublicRequestLimits::from_lookup(|name| {
            (name == "PUBLIC_MAX_CONNECTIONS").then(|| "1".into())
        })
        .unwrap();
        let budget = ConnectionBudget::new(limits);
        let app = axum::Router::new().route("/healthz", axum::routing::get(|| async { "healthy" }));
        let (stop, stopped) = watch::channel(false);
        let public_server =
            tokio::spawn(public.serve(app.clone(), stopped.clone(), budget.clone()));
        let api_server = tokio::spawn(PublicListener::Tcp(api).serve(app, stopped, budget.clone()));
        let mut held = tokio::net::UnixStream::connect(&path).await.unwrap();
        held.write_all(b"GET /healthz HTTP/1.1\r\n").await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while budget.admission.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let mut excess = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut response = Vec::new();
        let closed =
            tokio::time::timeout(Duration::from_secs(2), excess.read_to_end(&mut response))
                .await
                .expect("the API must reject a socket while the Unix connection holds admission");
        assert!(response.is_empty());
        assert!(
            closed.is_ok()
                || closed.is_err_and(|error| matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                ))
        );
        drop(held);
        tokio::time::timeout(Duration::from_secs(2), async {
            while budget.admission.available_permits() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let mut healthy = tokio::net::TcpStream::connect(address).await.unwrap();
        healthy
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            healthy.read_to_string(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 200") && response.ends_with("healthy"));
        stop.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), public_server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), api_server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn unix_shutdown_retains_the_owned_socket_until_requests_drain() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("public.sock");
        let listener = PublicListener::Unix(UnixListener::bind(&path).unwrap());
        let active = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let started = active.clone();
        let ready = release.clone();
        let app = axum::Router::new().route(
            "/hold",
            axum::routing::get(move || {
                let started = started.clone();
                let ready = ready.clone();
                async move {
                    started.notify_one();
                    ready.notified().await;
                    "drained"
                }
            }),
        );
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let mut server = tokio::spawn(listener.serve(
            app,
            stopped,
            ConnectionBudget::new(board_config::PublicRequestLimits::default()),
        ));
        let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
        client
            .write_all(b"GET /hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), active.notified())
            .await
            .unwrap();
        stop.send(true).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut server)
                .await
                .is_err(),
            "Unix listener stopped before its active request could drain"
        );
        assert!(path.exists());
        release.notify_one();
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.read_to_string(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.starts_with("HTTP/1.1 200") && response.ends_with("drained"));
        tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn real_socket_uses_kernel_uid_and_rejects_missing_or_duplicate_headers() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("public.sock");
        let actual = rustix::process::geteuid().as_raw();
        for expected in [actual, actual.wrapping_add(1)] {
            let listener = PublicListener::Unix(UnixListener::bind(&path).unwrap());
            let pool = sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                .unwrap();
            let (_, app, _) = crate::observed_routers_with_proxy(
                pool,
                "http://127.0.0.1:3000".into(),
                false,
                false,
                None,
                board_config::PublicRequestLimits::default(),
                Some(expected),
            );
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let server = tokio::spawn(listener.serve(
                app,
                stopped,
                ConnectionBudget::new(board_config::PublicRequestLimits::default()),
            ));
            for (headers, status) in [
                ("X-Board-Client-IP: 192.0.2.1\r\n", 200),
                ("", 400),
                ("X-Board-Client-IP: bad\r\n", 400),
                (
                    "X-Board-Client-IP: 192.0.2.1\r\nX-Board-Client-IP: 192.0.2.2\r\n",
                    400,
                ),
            ] {
                let mut client = tokio::net::UnixStream::connect(&path).await.unwrap();
                client.write_all(format!("GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n").as_bytes()).await.unwrap();
                let mut response = String::new();
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    client.read_to_string(&mut response),
                )
                .await
                .unwrap()
                .unwrap();
                let status = if expected == actual { status } else { 403 };
                assert!(
                    response.starts_with(&format!("HTTP/1.1 {status}")),
                    "{response}"
                );
            }
            stop.send(true).unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert!(!path.exists());
        }
    }

    #[tokio::test]
    async fn socket_permissions_cleanup_and_existing_paths_are_enforced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        let path = dir.path().join("public.sock");
        let socket = UnixListener::bind(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o660
        );
        assert!(UnixListener::bind(&path).is_err());
        drop(socket);
        assert!(!path.exists());
        std::fs::write(&path, "keep").unwrap();
        assert!(UnixListener::bind(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        std::fs::remove_file(&path).unwrap();
        let socket = UnixListener::bind(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "replacement").unwrap();
        drop(socket);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        std::fs::remove_file(&path).unwrap();
        symlink(dir.path().join("missing"), &path).unwrap();
        assert!(UnixListener::bind(&path).is_err());
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::remove_file(&path).unwrap();
        let alias = dir.path().join("alias");
        symlink(dir.path(), &alias).unwrap();
        assert!(UnixListener::bind(&alias.join("public.sock")).is_err());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(UnixListener::bind(&path).is_err());
    }
}
