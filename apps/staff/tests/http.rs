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
            media: None,
            proxy: None,
            poster_id_key: None,
            country_database: None,
            origin: "http://localhost:3001".into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: std::time::Duration::from_secs(900),
            tripcode_key: None,
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

#[cfg(target_os = "linux")]
#[tokio::test]
async fn actual_staff_unix_listener_requires_the_configured_kernel_uid_and_one_client_address() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (peer, _) = tokio::net::UnixStream::pair().unwrap();
    let actual = peer.peer_cred().unwrap().uid();
    for expected in [actual, actual.wrapping_add(1)] {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("staff.sock");
        let mut state = state();
        Arc::get_mut(&mut state).unwrap().config.proxy = board_config::PublicProxy::from_values(
            Some(socket.to_str().unwrap()),
            Some(&expected.to_string()),
            false,
        )
        .unwrap();
        let listener = board_http::transport::HttpListener::bind(
            state.config.bind,
            state.config.proxy.as_ref(),
        )
        .await
        .unwrap();
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(listener.serve(
            router(state),
            stopped,
            board_http::transport::ConnectionBudget::new(
                board_config::PublicRequestLimits::default(),
            ),
        ));
        for (headers, allowed_status) in [
            ("X-Board-Client-IP: ::ffff:192.0.2.1\r\n", 200),
            ("", 400),
            ("X-Board-Client-IP: 192.0.2.1:80\r\n", 400),
            (
                "X-Board-Client-IP: 192.0.2.1\r\nX-Board-Client-IP: 192.0.2.2\r\n",
                400,
            ),
        ] {
            let mut client = tokio::net::UnixStream::connect(&socket).await.unwrap();
            client
                .write_all(
                    format!(
                        "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut response = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                client.read_to_string(&mut response),
            )
            .await
            .unwrap()
            .unwrap();
            let status = if expected == actual {
                allowed_status
            } else {
                403
            };
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status}")),
                "Unexpected staff proxy status."
            );
            assert!(
                response
                    .to_lowercase()
                    .contains("cache-control: private, no-store\r\n")
            );
            assert!(!response.contains("192.0.2."));
        }
        stop.send(true).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!socket.exists());
    }
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
    for path in [
        "/reports",
        "/latest.php",
        "/j/latest.php",
        "/imgboard.php?mode=latest",
        "/j/imgboard.php?mode=latest",
    ] {
        for (cookie, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("staff=invalid"), StatusCode::UNAUTHORIZED),
            (
                Some("staff=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let mut request = Request::builder().uri(path);
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
}
#[tokio::test]
async fn state_changes_reject_origin_and_metadata_before_database() {
    for action in ["close", "remove-file"] {
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
                        .body(Body::from(format!(
                            "csrf=bad&board=test&target=1&action={action}"
                        )))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
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

#[tokio::test]
async fn enrollment_and_login_share_the_start_attempt_budget() {
    let app = app();
    for attempt in 0..32 {
        let enrollment = attempt % 2 == 0;
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(if enrollment {
                        "/enroll/start"
                    } else {
                        "/login/start"
                    })
                    .header("origin", "http://localhost:3001")
                    .header("sec-fetch-site", "same-origin")
                    .header("content-type", "application/json")
                    .body(Body::from(if enrollment {
                        r#"{"invitation":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#
                    } else {
                        r#"{"username":"synthetic"}"#
                    }))
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
