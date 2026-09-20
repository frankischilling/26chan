use super::{ConnectionBudget, PublicListener};
use axum::{
    Router,
    body::Body,
    routing::{get, post},
};
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, watch},
    task::JoinHandle,
};

const OUTER: Duration = Duration::from_secs(3);

struct DropNotify(Arc<Notify>);

impl Drop for DropNotify {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

async fn panic_response(started: Arc<Notify>) -> &'static str {
    started.notify_one();
    panic!("owned transport panic fixture");
}

struct ResponseChunks {
    bytes: bytes::Bytes,
    produced: Arc<std::sync::atomic::AtomicUsize>,
    cancelled: Arc<Notify>,
    remaining: usize,
}

impl futures_util::Stream for ResponseChunks {
    type Item = Result<bytes::Bytes, std::convert::Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.remaining == 0 {
            return std::task::Poll::Ready(None);
        }
        self.remaining -= 1;
        self.produced
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::task::Poll::Ready(Some(Ok(self.bytes.clone())))
    }
}

impl Drop for ResponseChunks {
    fn drop(&mut self) {
        self.cancelled.notify_one();
    }
}

fn limits(
    connections: usize,
    header_ms: u64,
    connection_ms: u64,
) -> board_config::PublicRequestLimits {
    board_config::PublicRequestLimits::from_lookup(|name| match name {
        "PUBLIC_MAX_CONNECTIONS" => Some(connections.to_string()),
        "PUBLIC_HEADER_TIMEOUT_MS" => Some(header_ms.to_string()),
        "PUBLIC_CONNECTION_TIMEOUT_MS" => Some(connection_ms.to_string()),
        _ => None,
    })
    .unwrap()
}

async fn listener() -> (PublicListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (PublicListener::Tcp(listener), address)
}

fn serve(
    listener: PublicListener,
    app: Router,
    stopped: watch::Receiver<bool>,
    budget: ConnectionBudget,
) -> JoinHandle<io::Result<()>> {
    tokio::spawn(listener.serve(app, stopped, budget))
}

async fn connect(address: SocketAddr) -> TcpStream {
    tokio::time::timeout(OUTER, TcpStream::connect(address))
        .await
        .expect("TCP connect timed out")
        .expect("TCP connect failed")
}

async fn request(stream: &mut TcpStream, path: &str, close: bool) {
    let connection = if close { "close" } else { "keep-alive" };
    tokio::time::timeout(
        OUTER,
        stream.write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: {connection}\r\n\r\n")
                .as_bytes(),
        ),
    )
    .await
    .expect("request write timed out")
    .expect("request write failed");
}

async fn response(stream: &mut TcpStream) -> String {
    tokio::time::timeout(OUTER, async {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            assert!(
                head.len() < 16 * 1024,
                "test response headers exceeded 16 KiB"
            );
            let mut byte = [0; 1];
            stream.read_exact(&mut byte).await?;
            head.push(byte[0]);
        }
        let header = std::str::from_utf8(&head)
            .expect("test response headers were not UTF-8")
            .to_owned();
        let content_length = header
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .expect("fixed test response omitted Content-Length");
        assert!(
            content_length <= 4096,
            "test response body unexpectedly large"
        );
        let mut body = vec![0; content_length];
        stream.read_exact(&mut body).await?;
        head.extend(body);
        Ok::<_, io::Error>(String::from_utf8(head).expect("test response was not UTF-8"))
    })
    .await
    .expect("response read timed out")
    .expect("response read failed")
}

async fn closed_bytes(stream: &mut TcpStream, within: Duration) -> usize {
    tokio::time::timeout(within, async {
        let mut total = 0usize;
        loop {
            let mut bytes = [0; 256];
            match stream.read(&mut bytes).await {
                Ok(0) => return total,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::BrokenPipe
                    ) =>
                {
                    return total;
                }
                Err(error) => panic!("unexpected connection read failure: {error}"),
                Ok(read) => {
                    total += read;
                    assert!(
                        total <= 8192,
                        "closed connection produced an unbounded response"
                    );
                }
            }
        }
    })
    .await
    .expect("connection remained open past its deadline")
}

async fn permits(budget: &ConnectionBudget, expected: usize) {
    tokio::time::timeout(OUTER, async {
        loop {
            if budget.admission.available_permits() == expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection permits did not reach the expected count");
}

async fn finish(server: JoinHandle<io::Result<()>>) {
    tokio::time::timeout(OUTER, server)
        .await
        .expect("server did not finish")
        .expect("server task failed")
        .expect("server returned an I/O error");
}

#[tokio::test]
async fn two_listeners_share_one_budget_and_close_excess_connections_without_parsing() {
    let (public, public_address) = listener().await;
    let (api, api_address) = listener().await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = Router::new().route(
        "/hold",
        get({
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    "held"
                }
            }
        }),
    );
    let budget = ConnectionBudget::new(limits(1, 1000, 2000));
    let (stop, stopped) = watch::channel(false);
    let public_server = serve(public, app.clone(), stopped.clone(), budget.clone());
    let api_server = serve(api, app, stopped, budget.clone());

    let mut active = connect(public_address).await;
    request(&mut active, "/hold", true).await;
    tokio::time::timeout(OUTER, started.notified())
        .await
        .expect("held request did not start");
    permits(&budget, 0).await;

    let mut excess = connect(api_address).await;
    assert_eq!(
        closed_bytes(&mut excess, Duration::from_millis(500)).await,
        0
    );
    assert_eq!(budget.admission.available_permits(), 0);

    release.notify_one();
    assert!(response(&mut active).await.starts_with("HTTP/1.1 200"));
    permits(&budget, 1).await;
    stop.send(true).unwrap();
    finish(public_server).await;
    finish(api_server).await;
}

#[tokio::test]
async fn disconnect_and_header_deadlines_release_capacity_for_a_healthy_request() {
    let (listener, address) = listener().await;
    let app = Router::new().route("/", get(|| async { "ok" }));
    // The total lifetime exceeds the outer test deadline, so it cannot hide
    // missing header timeout enforcement.
    let budget = ConnectionBudget::new(limits(1, 500, 10000));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let disconnected = connect(address).await;
    permits(&budget, 0).await;
    drop(disconnected);
    permits(&budget, 1).await;

    let mut silent = connect(address).await;
    permits(&budget, 0).await;
    let _ = closed_bytes(&mut silent, OUTER).await;
    permits(&budget, 1).await;

    let mut partial = connect(address).await;
    permits(&budget, 0).await;
    partial
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Incomplete:")
        .await
        .unwrap();
    let _ = closed_bytes(&mut partial, OUTER).await;
    permits(&budget, 1).await;

    let mut healthy = connect(address).await;
    request(&mut healthy, "/", true).await;
    let response = response(&mut healthy).await;
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.ends_with("ok"));
    permits(&budget, 1).await;
    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn absolute_lifetime_expires_a_silent_connection_before_its_header_deadline() {
    let (listener, address) = listener().await;
    let app = Router::new().route("/", get(|| async { "ok" }));
    let budget = ConnectionBudget::new(limits(1, 10000, 500));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());
    let mut silent = connect(address).await;
    permits(&budget, 0).await;
    assert_eq!(closed_bytes(&mut silent, OUTER).await, 0);
    permits(&budget, 1).await;
    let mut healthy = connect(address).await;
    request(&mut healthy, "/", true).await;
    assert!(response(&mut healthy).await.ends_with("ok"));
    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn incomplete_request_body_expires_after_headers_and_service_recovers() {
    let (listener, address) = listener().await;
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let app = Router::new().route("/", get(|| async { "ok" })).route(
        "/body",
        post({
            let started = started.clone();
            let cancelled = cancelled.clone();
            move |body: Body| {
                let started = started.clone();
                let cancelled = cancelled.clone();
                async move {
                    let _drop = DropNotify(cancelled);
                    started.notify_one();
                    match axum::body::to_bytes(body, 8).await {
                        Ok(_) => axum::http::StatusCode::OK,
                        Err(_) => axum::http::StatusCode::BAD_REQUEST,
                    }
                }
            }
        }),
    );
    let budget = ConnectionBudget::new(limits(1, 10000, 500));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());
    let mut client = connect(address).await;
    client
        .write_all(b"POST /body HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\n\r\na")
        .await
        .unwrap();
    tokio::time::timeout(OUTER, started.notified())
        .await
        .unwrap();
    permits(&budget, 0).await;
    tokio::time::timeout(OUTER, cancelled.notified())
        .await
        .expect("connection timeout did not cancel the incomplete request body");
    let _ = closed_bytes(&mut client, OUTER).await;
    permits(&budget, 1).await;
    let mut healthy = connect(address).await;
    request(&mut healthy, "/", true).await;
    assert!(response(&mut healthy).await.ends_with("ok"));
    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn blocked_response_writes_expire_and_release_connection_capacity() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (listener, address) = listener().await;
    let produced = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(Notify::new());
    let app = Router::new().route("/", get(|| async { "ok" })).route(
        "/stream",
        get({
            let produced = produced.clone();
            let cancelled = cancelled.clone();
            move || {
                let produced = produced.clone();
                let cancelled = cancelled.clone();
                async move {
                    Body::from_stream(ResponseChunks {
                        bytes: bytes::Bytes::from(vec![b'x'; 8192]),
                        produced,
                        cancelled,
                        remaining: 4096,
                    })
                }
            }
        }),
    );
    let budget = ConnectionBudget::new(limits(2, 10000, 1500));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(1024).unwrap();
    let mut blocked = socket.connect(address).await.unwrap();
    request(&mut blocked, "/stream", false).await;
    // Leave the client unread. The finite stream reuses one allocation, and
    // its producer must stop advancing when the actual socket fills.
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let before = produced.load(Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(25)).await;
            if before >= 2 && before == produced.load(Ordering::Relaxed) {
                break;
            }
        }
    })
    .await
    .expect("response producer never reached socket backpressure");
    assert!(produced.load(Ordering::Relaxed) < 4096);
    assert_eq!(budget.admission.available_permits(), 1);
    let mut healthy = connect(address).await;
    request(&mut healthy, "/", true).await;
    assert!(response(&mut healthy).await.ends_with("ok"));
    permits(&budget, 1).await;
    tokio::time::timeout(OUTER, cancelled.notified())
        .await
        .expect("connection timeout did not drop the blocked response body");
    permits(&budget, 2).await;
    drop(blocked);
    let mut recovered = connect(address).await;
    request(&mut recovered, "/", true).await;
    assert!(response(&mut recovered).await.ends_with("ok"));
    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn keep_alive_reuses_within_one_absolute_lifetime_and_does_not_reset_it() {
    let (listener, address) = listener().await;
    let app = Router::new()
        .route("/one", get(|| async { "one" }))
        .route("/two", get(|| async { "two" }));
    let budget = ConnectionBudget::new(limits(1, 10000, 1500));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut client = connect(address).await;
    request(&mut client, "/one", false).await;
    assert!(response(&mut client).await.ends_with("one"));
    tokio::time::sleep(Duration::from_millis(750)).await;
    request(&mut client, "/two", false).await;
    assert!(response(&mut client).await.ends_with("two"));

    // The original lifetime has at most 750 ms left. Resetting it for the
    // second request would keep it open another 1500 ms and fail this bound.
    let _ = closed_bytes(&mut client, Duration::from_millis(1100)).await;
    permits(&budget, 1).await;
    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn idle_keep_alive_retires_before_the_hard_deadline() {
    let (listener, address) = listener().await;
    let app = Router::new().route("/", get(|| async { "ok" }));
    let budget = ConnectionBudget::new(limits(1, 10000, 4000));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut client = connect(address).await;
    permits(&budget, 0).await;
    let admitted = tokio::time::Instant::now();
    request(&mut client, "/", false).await;
    assert!(response(&mut client).await.ends_with("ok"));

    // The 4 second hard lifetime has a 500 ms retirement window. An idle
    // keep-alive connection must close inside that window rather than remaining
    // available for a pooled client to reuse right up to the hard cutoff.
    tokio::time::sleep_until(admitted + Duration::from_millis(3000)).await;
    assert_eq!(
        closed_bytes(&mut client, Duration::from_millis(600)).await,
        0,
        "idle keep-alive connection survived past its retirement window"
    );
    permits(&budget, 1).await;

    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn keep_alive_retires_before_the_hard_deadline_after_draining_an_inflight_request() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (listener, address) = listener().await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let completed = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route("/one", get(|| async { "one" })).route(
        "/hold",
        post({
            let started = started.clone();
            let release = release.clone();
            let completed = completed.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                let completed = completed.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    completed.fetch_add(1, Ordering::Relaxed);
                    "drained"
                }
            }
        }),
    );
    let budget = ConnectionBudget::new(limits(1, 10000, 4000));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut client = connect(address).await;
    permits(&budget, 0).await;
    let admitted = tokio::time::Instant::now();
    request(&mut client, "/one", false).await;
    assert!(response(&mut client).await.ends_with("one"));

    // The 4 second hard lifetime leaves a 500 ms retirement window. Start a
    // request before that window, keep it active across the soft cutoff, then
    // let it finish. Graceful retirement must deliver this response and close
    // the keep-alive connection instead of leaving it reusable until 4 seconds.
    tokio::time::sleep_until(admitted + Duration::from_millis(3100)).await;
    client
        .write_all(
            b"POST /hold HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",
        )
        .await
        .unwrap();
    tokio::time::timeout(OUTER, started.notified())
        .await
        .expect("held request did not start before retirement");
    tokio::time::sleep_until(admitted + Duration::from_millis(3600)).await;
    release.notify_one();
    assert!(response(&mut client).await.ends_with("drained"));
    assert_eq!(completed.load(Ordering::Relaxed), 1);
    assert_eq!(
        closed_bytes(&mut client, Duration::from_millis(200)).await,
        0,
        "retired keep-alive connection remained reusable near its hard deadline"
    );
    permits(&budget, 1).await;

    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn absolute_connection_deadline_cancels_a_held_handler_and_releases_its_permit() {
    let (listener, address) = listener().await;
    let started = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let app = Router::new().route(
        "/hang",
        get({
            let started = started.clone();
            let cancelled = cancelled.clone();
            move || {
                let started = started.clone();
                let cancelled = cancelled.clone();
                async move {
                    let _drop = DropNotify(cancelled);
                    started.notify_one();
                    std::future::pending::<()>().await;
                }
            }
        }),
    );
    let budget = ConnectionBudget::new(limits(1, 10000, 500));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut client = connect(address).await;
    request(&mut client, "/hang", false).await;
    tokio::time::timeout(OUTER, started.notified())
        .await
        .expect("held handler did not start");
    permits(&budget, 0).await;
    tokio::time::timeout(OUTER, cancelled.notified())
        .await
        .expect("connection deadline did not cancel the handler");
    let _ = closed_bytes(&mut client, OUTER).await;
    permits(&budget, 1).await;

    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn aborting_the_parent_serving_future_drops_all_owned_connections_and_permits() {
    let (listener, address) = listener().await;
    let app = Router::new().route("/", get(|| async { "ok" }));
    let budget = ConnectionBudget::new(limits(2, 5000, 5000));
    let (_stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut first = connect(address).await;
    permits(&budget, 1).await;
    let mut second = connect(address).await;
    permits(&budget, 0).await;

    server.abort();
    let error = tokio::time::timeout(OUTER, server)
        .await
        .expect("aborted server task did not settle")
        .expect_err("aborted server unexpectedly completed normally");
    assert!(error.is_cancelled());
    permits(&budget, 2).await;
    let _ = closed_bytes(&mut first, OUTER).await;
    let _ = closed_bytes(&mut second, OUTER).await;
}

#[tokio::test]
async fn handler_panic_closes_only_its_connection_and_capacity_serves_the_next_request() {
    let (listener, address) = listener().await;
    let panic_started = Arc::new(Notify::new());
    let app = Router::new()
        .route(
            "/panic",
            get({
                let started = panic_started.clone();
                move || panic_response(started.clone())
            }),
        )
        .route("/healthy", get(|| async { "healthy" }));
    let budget = ConnectionBudget::new(limits(1, 1000, 2000));
    let (stop, stopped) = watch::channel(false);
    let server = serve(listener, app, stopped, budget.clone());

    let mut panicked = connect(address).await;
    request(&mut panicked, "/panic", false).await;
    tokio::time::timeout(OUTER, panic_started.notified())
        .await
        .unwrap();
    let _ = closed_bytes(&mut panicked, OUTER).await;
    permits(&budget, 1).await;

    let mut healthy = connect(address).await;
    request(&mut healthy, "/healthy", true).await;
    let response = response(&mut healthy).await;
    assert!(response.starts_with("HTTP/1.1 200") && response.ends_with("healthy"));
    permits(&budget, 1).await;

    stop.send(true).unwrap();
    finish(server).await;
}

#[tokio::test]
async fn stop_drains_a_released_handler_but_the_original_deadline_bounds_a_stalled_one() {
    let (listener, address) = listener().await;
    let released_started = Arc::new(Notify::new());
    let stalled_started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let stalled_cancelled = Arc::new(Notify::new());
    let app = Router::new()
        .route(
            "/release",
            get({
                let started = released_started.clone();
                let release = release.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    async move {
                        started.notify_one();
                        release.notified().await;
                        "drained"
                    }
                }
            }),
        )
        .route(
            "/stall",
            get({
                let started = stalled_started.clone();
                let cancelled = stalled_cancelled.clone();
                move || {
                    let started = started.clone();
                    let cancelled = cancelled.clone();
                    async move {
                        let _drop = DropNotify(cancelled);
                        started.notify_one();
                        std::future::pending::<()>().await;
                    }
                }
            }),
        );
    let budget = ConnectionBudget::new(limits(2, 10000, 1000));
    let (stop, stopped) = watch::channel(false);
    let mut server = serve(listener, app, stopped, budget.clone());

    let mut released = connect(address).await;
    request(&mut released, "/release", true).await;
    tokio::time::timeout(OUTER, released_started.notified())
        .await
        .expect("releasable handler did not start");
    let mut stalled = connect(address).await;
    request(&mut stalled, "/stall", false).await;
    tokio::time::timeout(OUTER, stalled_started.notified())
        .await
        .expect("stalled handler did not start");
    permits(&budget, 0).await;

    stop.send(true).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut server)
            .await
            .is_err(),
        "server stopped before active requests had a chance to drain"
    );
    release.notify_one();
    let response = response(&mut released).await;
    assert!(response.starts_with("HTTP/1.1 200") && response.ends_with("drained"));

    tokio::time::timeout(OUTER, stalled_cancelled.notified())
        .await
        .expect("original connection deadline did not cancel stalled graceful shutdown");
    let _ = closed_bytes(&mut stalled, OUTER).await;
    tokio::time::timeout(OUTER, server)
        .await
        .expect("server did not finish after bounded drain")
        .expect("server task failed")
        .expect("server returned an I/O error");
    permits(&budget, 2).await;
}
