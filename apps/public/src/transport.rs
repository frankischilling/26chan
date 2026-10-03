#[cfg(target_os = "linux")]
pub use board_http::transport::UnixListener;
pub use board_http::transport::{ConnectionBudget, HttpListener as PublicListener};

#[cfg(all(test, target_os = "linux"))]
mod unix_proxy_tests {
    use super::*;

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
}
