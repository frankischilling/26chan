use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn offline_app() -> axum::Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    board_public::router(pool, "http://127.0.0.1:3000".into(), false)
}

#[tokio::test]
async fn retained_public_responses_keep_the_admission_budget() {
    let app = offline_app();
    let mut held = Vec::new();
    for _ in 0..32 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    let busy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(busy.headers()["cache-control"], "no-store");
    drop(held.pop());
    let recovered = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
    let mut body = recovered.into_body();
    let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(&data[..], b"ok");
    let slice = data.slice(0..1);
    drop(body);
    drop(data);
    let still_busy = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(still_busy.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(slice);
    let recovered = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
}

#[tokio::test]
async fn integer_page_extremes_are_rejected_without_panics() {
    for path in [
        "/test/9223372036854775807",
        "/test/-9223372036854775808",
        "/test/1001",
    ] {
        let response = offline_app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn streamed_form_limit_applies_without_content_length() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, offline_app()).await.unwrap();
    });
    for (size, status) in [(262_144, "422"), (262_145, "413")] {
        let response = tokio::task::spawn_blocking(move || {
            use std::io::{Read, Write};
            let mut socket = std::net::TcpStream::connect(address).unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            socket.set_write_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            // Transfer-Encoding keeps the body length unknown to the server.
            // Send multiple frames across the actual HTTP transport. A body
            // exactly at the limit reaches form validation (missing fields).
            write!(socket, "POST /test/post HTTP/1.1\r\nHost: {address}\r\nOrigin: http://127.0.0.1:3000\r\nContent-Type: application/x-www-form-urlencoded\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            for chunk in vec![b'x'; size].chunks(8192) {
                write!(socket, "{:X}\r\n", chunk.len()).unwrap();
                socket.write_all(chunk).unwrap();
                socket.write_all(b"\r\n").unwrap();
            }
            socket.write_all(b"0\r\n\r\n").unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            response
        }).await.unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status} ")),
            "unexpected response: {response}"
        );
        assert!(response.contains("x-content-type-options: nosniff\r\n"));
        assert!(response.contains("cache-control: no-store\r\n"));
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn percent_encoded_comment_budget_reaches_form_validation() {
    // A missing password must reach form validation even with a comment that
    // needs the full 192,000-byte URL-encoding budget. No database is needed.
    let response = offline_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/post")
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("com={}", "%F0%9F%98%80".repeat(16_000))))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn forwarding_headers_do_not_evade_write_rate_limits() {
    let app = offline_app();
    for i in 0..31 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/test/post")
                    .header("origin", "http://127.0.0.1:3000")
                    .header("x-forwarded-for", format!("192.0.2.{i}"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from("malformed=form"))
                    .unwrap(),
            )
            .await
            .unwrap();
        if i < 30 {
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        } else {
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
    }
}

#[tokio::test]
async fn unavailable_storage_never_allows_deletion_and_staff_routes_are_absent() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    pool.close().await;
    let app = board_public::router(pool, "http://127.0.0.1:3000".into(), false);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/delete")
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("no=1&password=test-password"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/staff/admin/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unsupported_media_does_not_accept_bytes() {
    let response = offline_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/upload")
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "image/png")
                .body(Body::from("harmless synthetic bytes"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
