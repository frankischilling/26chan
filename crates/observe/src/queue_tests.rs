use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, StatusCode},
};
use std::sync::{Mutex, atomic::AtomicUsize};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn config() -> Config {
    Config::parse(Some("127.0.0.1:9194"), Some(TOKEN))
        .unwrap()
        .unwrap()
}

fn request(method: Method, path: &str, authenticated: bool) -> Request {
    let mut builder = Request::builder().method(method).uri(path);
    if authenticated {
        builder = builder.header("authorization", format!("Bearer {TOKEN}"));
    }
    builder.body(Body::empty()).unwrap()
}

fn populated() -> MediaQueueSample {
    MediaQueueSample {
        available: true,
        last_success_timestamp_seconds: 1_800_000_000,
        capacity: 32,
        active: [1, 2, 3],
        expired: [4, 5, 6],
        oldest_queued_seconds: 77,
        failures_recent: [7, 8, 9, 10, 11],
    }
}

fn media_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with("board_media_"))
        .collect()
}

#[test]
fn queue_registration_is_optional_unique_and_precedes_sharing() {
    let mut metrics = Metrics::new();
    assert!(!metrics.render().contains("board_media_"));
    metrics.register_media_queue(populated).unwrap();
    assert!(
        metrics
            .register_media_queue(MediaQueueSample::default)
            .is_err()
    );
    let mut other = Metrics::new();
    let shared = other.clone();
    assert!(other.register_media_queue(populated).is_err());
    assert!(!shared.render().contains("board_media_"));
}

#[test]
fn queue_exposition_uses_one_snapshot_and_only_the_fixed_state_and_reason_labels() {
    let mut metrics = Metrics::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let callback_reads = reads.clone();
    metrics
        .register_media_queue(move || {
            callback_reads.fetch_add(1, Ordering::Relaxed);
            populated()
        })
        .unwrap();
    let text = metrics.render();
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    assert_eq!(
        media_lines(&text),
        vec![
            "board_media_sample_success 1",
            "board_media_sample_last_success_timestamp_seconds 1800000000",
            "board_media_queue_capacity 32",
            "board_media_jobs{state=\"receiving\"} 1",
            "board_media_jobs{state=\"queued\"} 2",
            "board_media_jobs{state=\"processing\"} 3",
            "board_media_expired_jobs{state=\"receiving\"} 4",
            "board_media_expired_jobs{state=\"queued\"} 5",
            "board_media_expired_jobs{state=\"processing\"} 6",
            "board_media_oldest_queued_seconds 77",
            "board_media_failures_recent{reason=\"intake_failed\"} 7",
            "board_media_failures_recent{reason=\"abandoned\"} 8",
            "board_media_failures_recent{reason=\"processing_failed\"} 9",
            "board_media_failures_recent{reason=\"invalid_output\"} 10",
            "board_media_failures_recent{reason=\"retry_exhausted\"} 11",
        ]
    );
    for family in [
        "sample_success",
        "sample_last_success_timestamp_seconds",
        "queue_capacity",
        "jobs",
        "expired_jobs",
        "oldest_queued_seconds",
        "failures_recent",
    ] {
        assert!(text.contains(&format!("# TYPE board_media_{family} gauge\n")));
    }
}

#[test]
fn failed_snapshot_omits_queue_values_preserves_last_success_and_recovers() {
    let current = Arc::new(Mutex::new(MediaQueueSample::default()));
    let sampled = current.clone();
    let mut metrics = Metrics::new();
    metrics
        .register_media_queue(move || *sampled.lock().unwrap())
        .unwrap();
    assert_eq!(
        media_lines(&metrics.render()),
        [
            "board_media_sample_success 0",
            "board_media_sample_last_success_timestamp_seconds 0"
        ]
    );
    *current.lock().unwrap() = populated();
    assert!(metrics.render().contains("board_media_queue_capacity 32\n"));
    current.lock().unwrap().available = false;
    let text = metrics.render();
    assert_eq!(
        media_lines(&text),
        [
            "board_media_sample_success 0",
            "board_media_sample_last_success_timestamp_seconds 1800000000"
        ]
    );
    for absent in [
        "board_media_queue_capacity",
        "board_media_jobs",
        "board_media_expired_jobs",
        "board_media_oldest_queued_seconds",
        "board_media_failures_recent",
    ] {
        assert!(
            !text.contains(absent),
            "unavailable data must omit {absent}"
        );
    }
    current.lock().unwrap().available = true;
    assert!(
        metrics
            .render()
            .contains("board_media_jobs{state=\"processing\"} 3\n")
    );
}

#[tokio::test]
async fn optional_health_routes_authenticate_and_read_only_the_readiness_callback() {
    let ready = Arc::new(AtomicBool::new(false));
    let callback_ready = ready.clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let callback_reads = reads.clone();
    let app = exporter_router_with_health(
        Metrics::new(),
        config(),
        Some(Arc::new(move || {
            callback_reads.fetch_add(1, Ordering::Relaxed);
            callback_ready.load(Ordering::Relaxed)
        })),
    );
    for path in ["/healthz", "/readyz"] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, path, false))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    for (path, expected) in [
        ("/healthz", StatusCode::OK),
        ("/readyz", StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let response = app
            .clone()
            .oneshot(request(Method::GET, path, true))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(
            to_bytes(response.into_body(), 100)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    ready.store(true, Ordering::Relaxed);
    assert_eq!(
        app.clone()
            .oneshot(request(Method::HEAD, "/readyz", true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request(Method::POST, "/readyz", true))
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(reads.load(Ordering::Relaxed), 2);
    let old = exporter_router(Metrics::new(), config());
    for path in ["/healthz", "/readyz"] {
        assert_eq!(
            old.clone()
                .oneshot(request(Method::GET, path, true))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn health_router_preserves_retained_scrape_limit_and_private_fixed_series() {
    let mut metrics = Metrics::new();
    metrics.register_media_queue(populated).unwrap();
    let app = exporter_router_with_health(metrics, config(), Some(Arc::new(|| true)));
    let mut retained = Vec::new();
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(request(Method::GET, "/metrics?private-job=secret", true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        retained.push(response);
    }
    assert_eq!(
        app.clone()
            .oneshot(request(Method::GET, "/metrics", true))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        app.clone()
            .oneshot(request(Method::GET, "/healthz", true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let body = to_bytes(retained.pop().unwrap().into_body(), 32_768)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    for secret in [
        TOKEN,
        "private",
        "secret",
        "job_id",
        "filename",
        "hash",
        "lease_token",
    ] {
        assert!(!text.contains(secret));
    }
    drop(body);
    assert_eq!(
        app.clone()
            .oneshot(request(Method::GET, "/metrics", true))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn optional_health_endpoint_uses_the_same_bound_socket_and_paired_lifecycle() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut config = config();
    config.bind.set_port(0);
    let endpoint = Endpoint::bind(Some(config)).await.unwrap();
    let address = endpoint.0.as_ref().unwrap().0.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(endpoint.serve_with_health(
        Metrics::new(),
        async {
            stopped.await.unwrap();
            Ok(())
        },
        || false,
    ));
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client
        .write_all(
            format!(
                "GET /readyz HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut reply = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.read_to_end(&mut reply),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 503 Service Unavailable\r\n"));
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
