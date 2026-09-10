use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use http_body_util::BodyExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn non_unicode_environment_values_fail_without_echoing_values() {
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    };
    assert!(Config::from_os_values(Some(invalid.clone()), Some(TOKEN.into())).is_err());
    assert!(Config::from_os_values(Some("127.0.0.1:9100".into()), Some(invalid)).is_err());
    assert!(Config::from_os_values(None, None).unwrap().is_none());
}

#[test]
fn config_is_optional_but_requires_a_complete_loopback_configuration() {
    assert!(Config::parse(None, None).unwrap().is_none());
    for address in ["127.0.0.1:9100", "127.42.0.1:9100", "[::1]:9100"] {
        let config = Config::parse(Some(address), Some(TOKEN)).unwrap().unwrap();
        assert_eq!(config.bind, address.parse::<SocketAddr>().unwrap());
    }
    for (address, token) in [
        (Some("127.0.0.1:9100"), None),
        (None, Some(TOKEN)),
        (Some("0.0.0.0:9100"), Some(TOKEN)),
        (Some("[::]:9100"), Some(TOKEN)),
        (Some("192.0.2.1:9100"), Some(TOKEN)),
        (Some("[::ffff:127.0.0.1]:9100"), Some(TOKEN)),
        (Some("127.0.0.1:0"), Some(TOKEN)),
        (Some("localhost:9100"), Some(TOKEN)),
        (Some("bad-secret-address"), Some(TOKEN)),
        (Some("127.0.0.1:9100"), Some("")),
        (Some("127.0.0.1:9100"), Some("short-secret")),
    ] {
        let error = Config::parse(address, token).unwrap_err();
        let rendered = format!("{error:?}");
        for secret in [TOKEN, "bad-secret-address", "short-secret"] {
            assert!(!rendered.contains(secret));
        }
    }
    for token in [
        TOKEN.to_uppercase(),
        format!("{TOKEN} "),
        "g".repeat(64),
        "0".repeat(63),
    ] {
        assert!(Config::parse(Some("127.0.0.1:9100"), Some(&token)).is_err());
    }
    let config = Config::parse(Some("127.0.0.1:9100"), Some(TOKEN))
        .unwrap()
        .unwrap();
    assert!(!format!("{config:?}").contains(TOKEN));
}

fn request(method: Method, uri: &str) -> Request {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn sample<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{name} ")))
        .unwrap_or_else(|| panic!("missing {name} in {text}"))
}

#[tokio::test]
async fn counts_final_status_rejections_and_missing_routes_without_private_labels() {
    let metrics = Metrics::new();
    let app = Router::new().route(
        "/status/{code}",
        any(
            |axum::extract::Path(code): axum::extract::Path<u16>| async move {
                StatusCode::from_u16(code).unwrap()
            },
        ),
    );
    let app = metrics.layer(app, Listener::Public);
    for (method, code) in [
        (Method::GET, 103),
        (Method::GET, 200),
        (Method::GET, 302),
        (Method::GET, 401),
        (Method::POST, 403),
        (Method::PUT, 429),
        (Method::PATCH, 500),
        (Method::DELETE, 503),
        (Method::GET, 600),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                method,
                &format!("/status/{code}?secret-query=private"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), code);
    }
    assert_eq!(
        app.oneshot(request(Method::GET, "/metrics?secret-path=private"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let text = metrics.render();
    for (class, count) in [
        ("1xx", "1"),
        ("2xx", "1"),
        ("3xx", "1"),
        ("4xx", "4"),
        ("5xx", "2"),
        ("other", "1"),
    ] {
        assert_eq!(
            sample(
                &text,
                &format!(
                    "board_http_responses_total{{listener=\"public\",status_class=\"{class}\"}}"
                )
            ),
            count
        );
    }
    for (name, count) in [
        ("write_rejections_total", "4"),
        ("authorization_rejections_total", "2"),
        ("capacity_responses_total", "1"),
        ("cancelled_total", "0"),
        ("handlers_inflight", "0"),
        ("handler_duration_seconds_count", "10"),
    ] {
        assert_eq!(
            sample(&text, &format!("board_http_{name}{{listener=\"public\"}}")),
            count
        );
    }
    assert_eq!(
        sample(
            &text,
            "board_http_handler_duration_seconds_bucket{listener=\"public\",le=\"+Inf\"}"
        ),
        "10"
    );
    for secret in [
        "secret",
        "private",
        "status/",
        "method=",
        TOKEN,
        "listener=\"api\"",
        "listener=\"staff\"",
        "listener=\"media\"",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
}

#[tokio::test]
async fn concurrent_requests_count_once_and_keep_bodies_and_extensions() {
    let metrics = Metrics::new();
    let app = Router::new().route(
        "/",
        get(|| async {
            let mut response = "payload".into_response();
            response.extensions_mut().insert(42u32);
            response
        }),
    );
    let app = metrics.layer(metrics.layer(app, Listener::Api), Listener::Api);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..100 {
        let app = app.clone();
        tasks.spawn(async move {
            let response = app.oneshot(request(Method::GET, "/")).await.unwrap();
            assert_eq!(response.extensions().get::<u32>(), Some(&42));
            assert_eq!(
                to_bytes(response.into_body(), 100).await.unwrap(),
                "payload"
            );
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    let text = metrics.render();
    assert_eq!(
        sample(
            &text,
            "board_http_responses_total{listener=\"api\",status_class=\"2xx\"}"
        ),
        "100"
    );
    assert_eq!(
        sample(&text, "board_http_handlers_inflight{listener=\"api\"}"),
        "0"
    );
    assert_eq!(
        sample(
            &text,
            "board_http_handler_duration_seconds_count{listener=\"api\"}"
        ),
        "100"
    );
}

#[tokio::test]
async fn cancelling_a_pending_handler_restores_inflight_without_a_completed_response() {
    let metrics = Metrics::new();
    let entered = Arc::new(tokio::sync::Notify::new());
    let signal = entered.clone();
    let app = metrics.layer(
        Router::new().route(
            "/",
            get(move || {
                let signal = signal.clone();
                async move {
                    signal.notify_one();
                    std::future::pending::<Response>().await
                }
            }),
        ),
        Listener::Media,
    );
    let task = tokio::spawn(app.oneshot(request(Method::GET, "/")));
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    assert_eq!(
        sample(
            &metrics.render(),
            "board_http_handlers_inflight{listener=\"media\"}"
        ),
        "1"
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let text = metrics.render();
    assert_eq!(
        sample(&text, "board_http_handlers_inflight{listener=\"media\"}"),
        "0"
    );
    assert_eq!(
        sample(&text, "board_http_cancelled_total{listener=\"media\"}"),
        "1"
    );
    assert_eq!(
        sample(
            &text,
            "board_http_handler_duration_seconds_count{listener=\"media\"}"
        ),
        "0"
    );
}

#[test]
fn pool_callbacks_are_live_fixed_labels_and_registration_is_final_before_sharing() {
    let mut metrics = Metrics::new();
    let size = Arc::new(AtomicU32::new(3));
    let sampled_size = size.clone();
    metrics
        .register_pool(Pool::StaffAuth, move || PoolSample {
            size: sampled_size.load(Ordering::Relaxed),
            idle: 2,
            max: 8,
        })
        .unwrap();
    let text = metrics.render();
    assert_eq!(
        sample(&text, "board_db_pool_connections{pool=\"staff_auth\"}"),
        "3"
    );
    assert_eq!(
        sample(&text, "board_db_pool_idle{pool=\"staff_auth\"}"),
        "2"
    );
    assert_eq!(
        sample(&text, "board_db_pool_max_connections{pool=\"staff_auth\"}"),
        "8"
    );
    assert!(!text.contains("listener="));
    size.store(5, Ordering::Relaxed);
    assert_eq!(
        sample(
            &metrics.render(),
            "board_db_pool_connections{pool=\"staff_auth\"}"
        ),
        "5"
    );
    assert!(
        metrics
            .register_pool(Pool::StaffAuth, || PoolSample {
                size: 0,
                idle: 0,
                max: 1
            })
            .is_err()
    );
    let shared = metrics.clone();
    assert!(
        metrics
            .register_pool(Pool::Public, || PoolSample {
                size: 0,
                idle: 0,
                max: 1
            })
            .is_err()
    );
    assert_eq!(shared.render(), metrics.render());
}

#[tokio::test]
async fn user_controlled_requests_cannot_grow_the_metric_schema() {
    let metrics = Metrics::new();
    let app = metrics.layer(Router::new(), Listener::Staff);
    let labels = |text: String| {
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.rsplit_once(' ').unwrap().0.to_owned())
            .collect::<Vec<_>>()
    };
    let before = labels(metrics.render());
    for i in 0..100 {
        let request = Request::builder()
            .uri(format!("/private-{i}?token={TOKEN}"))
            .header("cookie", "staff_session=private-session")
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::from("private-body"))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    let text = metrics.render();
    assert_eq!(before, labels(text.clone()));
    assert!(!text.contains("private"));
    assert!(!text.contains(TOKEN));
    assert!(text.len() < 32_768);
}

fn config() -> Config {
    Config::parse(Some("127.0.0.1:9100"), Some(TOKEN))
        .unwrap()
        .unwrap()
}

fn authenticated(method: Method, uri: &str) -> Request {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn exporter_rejects_missing_wrong_malformed_and_duplicate_authorization() {
    let app = exporter_router(Metrics::new(), config());
    let mut requests = vec![request(Method::GET, "/metrics")];
    for auth in [
        format!("Bearer {}", "f".repeat(64)),
        format!("bearer {TOKEN}"),
        format!("Bearer {TOKEN} "),
        format!("Bearer {TOKEN}, Bearer {TOKEN}"),
        format!("Basic {TOKEN}"),
    ] {
        requests.push(
            Request::builder()
                .uri("/metrics")
                .header("authorization", auth)
                .body(Body::empty())
                .unwrap(),
        );
    }
    for values in [
        [format!("Bearer {TOKEN}"), format!("Bearer {TOKEN}")],
        [format!("Bearer {TOKEN}"), "wrong".to_owned()],
        ["wrong".to_owned(), format!("Bearer {TOKEN}")],
    ] {
        let mut req = request(Method::GET, "/metrics");
        for value in values {
            req.headers_mut()
                .append("authorization", value.parse().unwrap());
        }
        requests.push(req);
    }
    for request in requests {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&body).contains(TOKEN));
        assert!(!String::from_utf8_lossy(&body).contains("board_"));
    }
}

#[tokio::test]
async fn exporter_serves_only_get_metrics_with_standard_head_and_cache_headers() {
    let app = exporter_router(Metrics::new(), config());
    let response = app
        .clone()
        .oneshot(authenticated(Method::GET, "/metrics"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        !response
            .headers()
            .contains_key("access-control-allow-origin")
    );
    assert!(
        to_bytes(response.into_body(), 32_768)
            .await
            .unwrap()
            .starts_with(b"# HELP")
    );
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(authenticated(Method::HEAD, "/metrics"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(
            to_bytes(response.into_body(), 32_768)
                .await
                .unwrap()
                .is_empty()
        );
    }
    for (method, uri, status) in [
        (Method::POST, "/metrics", StatusCode::METHOD_NOT_ALLOWED),
        (Method::OPTIONS, "/metrics", StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/", StatusCode::NOT_FOUND),
        (Method::GET, "/metrics/", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(authenticated(method, uri))
                .await
                .unwrap()
                .status(),
            status
        );
    }
}

#[tokio::test]
async fn four_retained_bodies_and_emitted_data_hold_export_admission_until_drop() {
    let app = exporter_router(Metrics::new(), config());
    let mut retained = Vec::new();
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(authenticated(Method::GET, "/metrics"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        retained.push(response);
    }
    let full = app
        .clone()
        .oneshot(authenticated(Method::GET, "/metrics"))
        .await
        .unwrap();
    assert_eq!(full.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(full.headers()["cache-control"], "no-store");
    let (parts, mut body) = retained.pop().unwrap().into_parts();
    let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let slice = data.slice(0..1);
    drop(body);
    drop(data);
    assert_eq!(
        app.clone()
            .oneshot(authenticated(Method::GET, "/metrics"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(slice);
    assert_eq!(
        app.clone()
            .oneshot(authenticated(Method::GET, "/metrics"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    drop(parts);
    drop(retained);
    assert_eq!(
        app.oneshot(authenticated(Method::GET, "/metrics"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn disabled_endpoint_awaits_and_preserves_the_application_result() {
    let endpoint = Endpoint::bind(None).await.unwrap();
    let result = endpoint
        .serve(Metrics::new(), async {
            Err(std::io::Error::other("synthetic application error"))
        })
        .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        "synthetic application error"
    );
}

#[tokio::test]
async fn bind_conflict_fails_before_the_application_is_started() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config::parse(
        Some(&listener.local_addr().unwrap().to_string()),
        Some(TOKEN),
    )
    .unwrap();
    assert!(Endpoint::bind(config).await.is_err());
}

#[tokio::test]
async fn paired_exporter_failure_drops_the_application_future() {
    struct Dropped(Arc<AtomicU32>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let drops = Arc::new(AtomicU32::new(0));
    let guard = Dropped(drops.clone());
    let application = async move {
        let _guard = guard;
        std::future::pending().await
    };
    let result = pair(application, async {
        Err(std::io::Error::other("synthetic exporter error"))
    })
    .await;
    assert_eq!(result.unwrap_err().to_string(), "synthetic exporter error");
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn real_listener_serves_authenticated_scrapes_and_closes_on_application_completion() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut config = config();
    config.bind.set_port(0); // The OS owns allocation; public parse still rejects zero.
    let endpoint = Endpoint::bind(Some(config)).await.unwrap();
    let address = endpoint.0.as_ref().unwrap().0.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(endpoint.serve(Metrics::new(), async {
        stopped.await.unwrap();
        Ok(())
    }));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(
        String::from_utf8(reply)
            .unwrap()
            .contains("# HELP board_http_responses_total")
    );
    let mut waiting = tokio::net::TcpStream::connect(address).await.unwrap();
    waiting
        .write_all(b"GET /metrics HTTP/1.1\r\n")
        .await
        .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(2), waiting.read_u8())
        .await
        .unwrap();
    assert!(closed.is_err());
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_histogram_snapshots_have_cumulative_buckets_and_matching_count() {
    let metrics = Metrics::new();
    let app = metrics.layer(
        Router::new().route("/", get(|| async { "ok" })),
        Listener::Public,
    );
    let worker = tokio::spawn(async move {
        for _ in 0..10_000 {
            app.clone()
                .oneshot(request(Method::GET, "/"))
                .await
                .unwrap();
        }
    });
    for _ in 0..1_000 {
        let text = metrics.render();
        let counts: Vec<u64> = ["0.01", "0.1", "1", "5", "10", "+Inf"].iter().map(|bound| sample(&text, &format!("board_http_handler_duration_seconds_bucket{{listener=\"public\",le=\"{bound}\"}}")).parse().unwrap()).collect();
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(
            *counts.last().unwrap(),
            sample(
                &text,
                "board_http_handler_duration_seconds_count{listener=\"public\"}"
            )
            .parse::<u64>()
            .unwrap()
        );
    }
    worker.await.unwrap();
}

#[tokio::test]
async fn sixteen_owned_connections_bound_acceptance_and_release_capacity_after_disconnect() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(serve_listener(
        listener,
        exporter_router(Metrics::new(), config()),
    ));
    let mut held = Vec::new();
    for _ in 0..16 {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET /metrics HTTP/1.1\r\n")
            .await
            .unwrap();
        held.push(stream);
    }
    let mut waiting = tokio::net::TcpStream::connect(address).await.unwrap();
    waiting.write_all(format!("GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(250), waiting.read_u8())
            .await
            .is_err(),
        "seventeenth connection must stay queued until capacity is released"
    );
    drop(held.pop());
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), waiting.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn connection_deadline_closes_incomplete_headers_and_service_recovers() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(serve_listener_with_deadline(
        listener,
        exporter_router(Metrics::new(), config()),
        Duration::from_millis(50),
    ));
    let mut stalled = tokio::net::TcpStream::connect(address).await.unwrap();
    stalled
        .write_all(b"GET /metrics HTTP/1.1\r\n")
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), stalled.read_u8())
            .await
            .expect("deadline must close incomplete headers")
            .is_err()
    );
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn exporter_closes_after_each_scrape_even_if_the_client_requests_keep_alive() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(serve_listener(
        listener,
        exporter_router(Metrics::new(), config()),
    ));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client.write_all(format!("GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\nConnection: keep-alive\r\n\r\n").as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut reply))
        .await
        .expect("one scrape must close the connection")
        .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 200 OK\r\n"));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
