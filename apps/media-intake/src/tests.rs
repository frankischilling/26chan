use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use tower::ServiceExt;

#[tokio::test]
async fn authentication_precedes_body_polling_and_downstream_work() {
    let access = Access {
        token: "a".repeat(64),
        staff_token: None,
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(forbidden_handler))
        .layer(middleware::from_fn_with_state(access, http::protect));
    for headers in [
        vec![],
        vec!["Bearer wrong".into()],
        vec![format!("Bearer {}", "b".repeat(64))], // Staff slot is absent.
        vec![format!("Bearer {}", "A".repeat(64))],
        vec![format!("Bearer {} ", "a".repeat(64))],
        vec![format!("Basic {}", "a".repeat(64))],
        vec!["Bearer a".into(), "Bearer b".into()],
    ] {
        let mut request = Request::post("/");
        for value in headers {
            request = request.header("authorization", value);
        }
        let body = Body::from_stream(futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
                panic!("unauthorized body polled")
            },
        ));
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
    }
}

#[tokio::test]
async fn duplicate_and_invalid_credentials_are_rejected_with_both_slots_enabled() {
    let public = format!("Bearer {}", "a".repeat(64));
    let staff = format!("Bearer {}", "b".repeat(64));
    let access = Access {
        token: "a".repeat(64),
        staff_token: Some("b".repeat(64)),
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(forbidden_handler))
        .layer(middleware::from_fn_with_state(access, http::protect));
    for headers in [
        vec![public.clone(), public.clone()],
        vec![staff.clone(), staff.clone()],
        vec![public.clone(), staff.clone()],
        vec![staff.clone(), public.clone()],
        vec![format!("{public}, {staff}")],
        vec![format!("Bearer {}", "c".repeat(64))],
        vec![format!("Bearer {}", "b".repeat(63))],
    ] {
        let mut request = Request::post("/");
        for value in headers {
            request = request.header("authorization", value);
        }
        let body = Body::from_stream(futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
                panic!("invalid credentials caused body polling")
            },
        ));
        assert_eq!(
            app.clone()
                .oneshot(request.body(body).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[test]
fn service_credentials_must_be_valid_and_distinct() {
    let public = "a".repeat(64);
    let staff = "b".repeat(64);
    assert!(config::valid_credentials(&public, None));
    assert!(config::valid_credentials(&public, Some(&staff)));
    assert!(!config::valid_credentials(&public, Some(&public)));
    for invalid in ["".into(), "a".repeat(63), "A".repeat(64), "g".repeat(64)] {
        assert!(!config::valid_credentials(&public, Some(&invalid)));
        assert!(!config::valid_credentials(&invalid, Some(&staff)));
    }
}

async fn forbidden_handler() -> StatusCode {
    panic!("unauthorized handler ran");
}

#[tokio::test]
async fn both_credentials_reject_browsers_and_share_response_admission() {
    let access = Access {
        token: "a".repeat(64),
        staff_token: Some("b".repeat(64)),
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(access, http::protect))
        .layer(middleware::from_fn(board_http::retain_response_body));
    let request = |token: &str| {
        Request::post("/").header("authorization", format!("Bearer {}", token.repeat(64)))
    };
    for token in ["a", "b"] {
        for name in [
            "origin",
            "sec-fetch-site",
            "sec-fetch-mode",
            "sec-fetch-dest",
            "sec-fetch-user",
        ] {
            let body = Body::from_stream(futures_util::stream::poll_fn(
                |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
                    panic!("browser request caused body polling")
                },
            ));
            let response = app
                .clone()
                .oneshot(request(token).header(name, "null").body(body).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }
    let mut bodies = Vec::new();
    for index in 0..8 {
        let response = app
            .clone()
            .oneshot(
                request(if index % 2 == 0 { "a" } else { "b" })
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        bodies.push(to_bytes(response.into_body(), 10).await.unwrap());
    }
    let response = app
        .clone()
        .oneshot(request("b").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
    bodies.clear();
    assert_eq!(
        app.oneshot(request("b").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn intake_metrics_use_only_the_fixed_listener_label() {
    let metrics = board_observe::Metrics::new();
    let app = metrics.layer(
        Router::new().route("/", post(|| async { "ok" })),
        board_observe::Listener::Intake,
    );
    assert_eq!(
        app.oneshot(Request::post("/").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert!(metrics.render().contains("listener=\"intake\""));
}
