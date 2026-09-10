use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, router};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower::ServiceExt;
use webauthn_rs::prelude::*;
fn app() -> axum::Router {
    router(state())
}
fn state() -> Arc<AppState> {
    let origin = Url::parse("http://localhost:3001").unwrap();
    let pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(100))
        .connect_lazy("postgres://unavailable@127.0.0.1:1/unavailable")
        .unwrap();
    Arc::new(AppState {
        config: Config {
            origin: "http://localhost:3001".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: std::time::Duration::from_secs(900),
        },
        auth: pool.clone(),
        staff: pool,
        webauthn: WebauthnBuilder::new("localhost", &origin)
            .unwrap()
            .build()
            .unwrap(),
        limits: board_staff::Limits::default(),
    })
}

#[tokio::test]
async fn staff_metrics_count_auth_and_capacity_without_exposing_request_data() {
    let (metrics, app) = board_staff::observed_router(state());
    let denied = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/reports?private=synthetic-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(denied.headers()["cache-control"], "private, no-store");
    drop(denied);
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    drop(missing);
    let mut held = Vec::new();
    for _ in 0..16 {
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    let busy = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    let snapshot = metrics.render();
    assert!(snapshot.contains("board_http_authorization_rejections_total{listener=\"staff\"} 1\n"));
    assert!(snapshot.contains("board_http_capacity_responses_total{listener=\"staff\"} 1\n"));
    assert!(snapshot.contains("board_db_pool_connections{pool=\"staff_auth\"}"));
    assert!(snapshot.contains("board_db_pool_connections{pool=\"staff_content\"}"));
    assert!(!snapshot.contains("synthetic-secret"));
    assert!(!snapshot.contains("unavailable"));
}

#[tokio::test]
async fn retained_staff_responses_keep_the_admission_budget() {
    let app = app();
    let mut held = Vec::new();
    for _ in 0..16 {
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    let busy = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy.headers()["cache-control"], "private, no-store");
    drop(held.pop());
    let recovered = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
    let mut body = recovered.into_body();
    let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(!data.is_empty());
    drop(body);
    let still_busy = app
        .clone()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(still_busy.status(), StatusCode::TOO_MANY_REQUESTS);
    drop(data);
    let recovered = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
}
#[tokio::test]
async fn unauthenticated_and_unavailable_sessions_fail_closed() {
    for (cookie, status) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some("staff=invalid"), StatusCode::UNAUTHORIZED),
        (
            Some("staff=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let mut request = Request::builder().uri("/reports");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        let response = app()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
    }
}
#[tokio::test]
async fn state_changes_reject_origin_and_metadata_before_database() {
    for (origin, site) in [
        (None, None),
        (Some("http://localhost:3000"), Some("same-site")),
        (Some("http://localhost:3001"), Some("cross-site")),
        (Some("http://localhost:3001"), None),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/moderate")
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(v) = origin {
            request = request.header("origin", v);
        }
        if let Some(v) = site {
            request = request.header("sec-fetch-site", v);
        }
        let response = app()
            .oneshot(
                request
                    .body(Body::from("csrf=bad&board=test&target=1&action=close"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn login_start_attempt_budget_is_bounded() {
    let app = app();
    for attempt in 0..31 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login/start")
                    .header("origin", "http://localhost:3001")
                    .header("sec-fetch-site", "same-origin")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":"synthetic"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if attempt < 30 {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
}
