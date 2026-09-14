//! Public transport owns the socket and obtains identity from the kernel.
use std::{io, net::SocketAddr};

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
        stopped: tokio::sync::watch::Receiver<bool>,
    ) -> io::Result<()> {
        match self {
            Self::Tcp(listener) => serve_tcp(listener, app, stopped).await,
            #[cfg(target_os = "linux")]
            Self::Unix(listener) => {
                // Keep the ownership guard until connections have drained.
                let UnixListener { listener, _guard } = listener;
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<crate::proxy_peer::UnixPeer>(),
                )
                .with_graceful_shutdown(wait_for_stop(stopped))
                .await
            }
        }
    }
}

pub async fn serve_tcp(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    stopped: tokio::sync::watch::Receiver<bool>,
) -> io::Result<()> {
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_for_stop(stopped))
    .await
}

async fn wait_for_stop(mut stopped: tokio::sync::watch::Receiver<bool>) {
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

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
            let server = tokio::spawn(listener.serve(app, stopped));
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
