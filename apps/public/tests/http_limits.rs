use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

fn offline_app() -> axum::Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    board_public::router(pool, "http://127.0.0.1:3000".into(), false)
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
    let response = offline_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test/post")
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("x".repeat(65_537)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["cache-control"], "no-store");
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
