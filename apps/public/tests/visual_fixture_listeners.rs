//! Real TCP/HTTP checks for the fixture-only transport contract. These tests
//! require local IPv4 and IPv6 loopback support; missing IPv6 is a failure, just
//! as it is for the default fixture, rather than a silently skipped test.
#[path = "../examples/visual/listeners.rs"]
mod listeners;

use std::{ffi::OsStr, io, net::Ipv4Addr, net::Ipv6Addr, net::SocketAddr, time::Duration};

use axum::{Router, routing::get};
use listeners::{MediaProfile, bind_media};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

#[test]
fn profile_is_finite_and_defaults_to_dual_loopback() {
    assert_eq!(
        MediaProfile::parse(None).unwrap(),
        MediaProfile::DualLoopback
    );
    assert_eq!(
        MediaProfile::parse(Some(OsStr::new("dual-loopback"))).unwrap(),
        MediaProfile::DualLoopback
    );
    assert_eq!(
        MediaProfile::parse(Some(OsStr::new("ipv4-only"))).unwrap(),
        MediaProfile::Ipv4Only
    );
    for invalid in [
        "",
        "DUAL-LOOPBACK",
        " ipv4-only",
        "ipv4-only ",
        "auto",
        "0.0.0.0",
        "::",
    ] {
        assert_eq!(
            MediaProfile::parse(Some(OsStr::new(invalid)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_profile_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(
        MediaProfile::parse(Some(OsStr::from_bytes(&[0xff])))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

async fn request(address: SocketAddr) -> String {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET /media HTTP/1.1\r\nHost: localhost:3004\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    })
    .await
    .expect("loopback HTTP request timed out")
}

#[tokio::test]
async fn dual_loopback_serves_identical_router_and_closes_both_listeners() {
    let bound = bind_media(MediaProfile::DualLoopback, 0)
        .await
        .expect("both loopback families are required");
    let addresses: Vec<_> = bound
        .iter()
        .map(|listener| listener.local_addr().unwrap())
        .collect();
    assert_eq!(addresses.len(), 2);
    assert_eq!(addresses[0].ip(), Ipv4Addr::LOCALHOST);
    assert_eq!(addresses[1].ip(), Ipv6Addr::LOCALHOST);
    assert_eq!(addresses[0].port(), addresses[1].port());
    let app = Router::new().route("/media", get(|| async { "shared synthetic media" }));
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(listeners::serve(
        bound
            .into_iter()
            .map(|listener| (listener, app.clone()))
            .collect(),
        async { stopped.await.map_err(io::Error::other) },
    ));
    for address in &addresses {
        let response = request(*address).await;
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("shared synthetic media"), "{response}");
    }
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    for address in addresses {
        assert!(
            TcpStream::connect(address).await.is_err(),
            "shutdown must close every listener"
        );
    }
}

#[tokio::test]
async fn diagnostic_profile_preserves_ipv4_only_workload() {
    let bound = bind_media(MediaProfile::Ipv4Only, 0).await.unwrap();
    assert_eq!(bound.len(), 1);
    let address = bound[0].local_addr().unwrap();
    assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
    // A separate IPv6 listener can still claim this exact port: the diagnostic
    // profile did not bind it (nor a wildcard IPv6 address).
    let ipv6 = TcpListener::bind((Ipv6Addr::LOCALHOST, address.port()))
        .await
        .unwrap();
    let app = Router::new().route("/media", get(|| async { "ipv4 diagnostic" }));
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(listeners::serve(
        bound
            .into_iter()
            .map(|listener| (listener, app.clone()))
            .collect(),
        async { stopped.await.map_err(io::Error::other) },
    ));
    assert!(request(address).await.ends_with("ipv4 diagnostic"));
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    drop(ipv6);
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn ipv6_bind_failure_is_visible_and_releases_partial_ipv4_bind() {
    let occupied = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
    let port = occupied.local_addr().unwrap().port();
    // Verify this is a usable IPv4 port before exercising the second-bind error.
    drop(
        TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap(),
    );
    let error = bind_media(MediaProfile::DualLoopback, port)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .expect("failed dual bind must release IPv4");
}

#[tokio::test]
async fn shutdown_signal_error_still_releases_listener() {
    let bound = bind_media(MediaProfile::Ipv4Only, 0).await.unwrap();
    let address = bound[0].local_addr().unwrap();
    let error = listeners::serve(
        bound
            .into_iter()
            .map(|listener| (listener, Router::new()))
            .collect(),
        async { Err(io::Error::other("synthetic signal failure")) },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "synthetic signal failure");
    TcpListener::bind(address).await.unwrap();
}

#[tokio::test]
async fn shutdown_closes_an_existing_keep_alive_connection() {
    let bound = bind_media(MediaProfile::Ipv4Only, 0).await.unwrap();
    let address = bound[0].local_addr().unwrap();
    let app = Router::new().route("/media", get(|| async { "keep-alive media" }));
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(listeners::serve(
        bound
            .into_iter()
            .map(|listener| (listener, app.clone()))
            .collect(),
        async { stopped.await.map_err(io::Error::other) },
    ));
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /media HTTP/1.1\r\nHost: localhost:3004\r\n\r\n")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut response = Vec::new();
        let mut byte = [0];
        while !response.ends_with(b"keep-alive media") {
            stream.read_exact(&mut byte).await.unwrap();
            response.push(byte[0]);
        }
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    })
    .await
    .expect("persistent request should finish");
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
    let mut remaining = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut remaining))
        .await
        .expect("graceful shutdown must close existing connection")
        .unwrap();
    assert!(remaining.is_empty());
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn bounded_shutdown_drops_pending_handler_and_streaming_body() {
    use std::{
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll},
    };
    use tokio::sync::Notify;

    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    struct PendingBody {
        _guard: DropFlag,
        polled: Arc<Notify>,
    }
    impl futures_util::Stream for PendingBody {
        type Item = Result<bytes::Bytes, io::Error>;
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.polled.notify_one();
            Poll::Pending
        }
    }

    let handler_dropped = Arc::new(AtomicBool::new(false));
    let body_dropped = Arc::new(AtomicBool::new(false));
    let handler_entered = Arc::new(Notify::new());
    let body_polled = Arc::new(Notify::new());
    let app = Router::new()
        .route(
            "/pending",
            get({
                let dropped = handler_dropped.clone();
                let entered = handler_entered.clone();
                move || {
                    let guard = DropFlag(dropped.clone());
                    let entered = entered.clone();
                    async move {
                        entered.notify_one();
                        std::future::pending::<()>().await;
                        drop(guard);
                        "unreachable"
                    }
                }
            }),
        )
        .route(
            "/body",
            get({
                let dropped = body_dropped.clone();
                let polled = body_polled.clone();
                move || {
                    let body = PendingBody {
                        _guard: DropFlag(dropped.clone()),
                        polled: polled.clone(),
                    };
                    async move { axum::body::Body::from_stream(body) }
                }
            }),
        );
    let bound = bind_media(MediaProfile::Ipv4Only, 0).await.unwrap();
    let address = bound[0].local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(listeners::serve(
        bound
            .into_iter()
            .map(|listener| (listener, app.clone()))
            .collect(),
        async { stopped.await.map_err(io::Error::other) },
    ));
    let mut handler_client = TcpStream::connect(address).await.unwrap();
    handler_client
        .write_all(b"GET /pending HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), handler_entered.notified())
        .await
        .unwrap();
    let mut body_client = TcpStream::connect(address).await.unwrap();
    body_client
        .write_all(b"GET /body HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), body_polled.notified())
        .await
        .unwrap();
    stop.send(()).unwrap();
    let error = tokio::time::timeout(Duration::from_secs(7), server)
        .await
        .expect("forced cleanup must be bounded")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    // The runtime is still alive here. Neither detached request nor body may
    // survive the supervisor returning, even though neither completes itself.
    assert!(handler_dropped.load(Ordering::SeqCst));
    assert!(body_dropped.load(Ordering::SeqCst));
    for mut client in [handler_client, body_client] {
        tokio::time::timeout(Duration::from_secs(3), client.read_to_end(&mut Vec::new()))
            .await
            .expect("forced shutdown must close client sockets")
            .unwrap();
    }
    assert!(TcpStream::connect(address).await.is_err());
}
