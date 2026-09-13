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
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(forbidden_handler))
        .layer(middleware::from_fn_with_state(access, http::protect));
    for headers in [vec![], vec!["Bearer wrong"], vec!["Bearer a", "Bearer b"]] {
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
async fn duplicate_valid_service_credentials_are_rejected() {
    let access = Access {
        token: "a".repeat(64),
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(forbidden_handler))
        .layer(middleware::from_fn_with_state(access, http::protect));
    let bearer = format!("Bearer {}", "a".repeat(64));
    let request = Request::post("/")
        .header("authorization", &bearer)
        .header("authorization", &bearer)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.oneshot(request).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}

async fn forbidden_handler() -> StatusCode {
    panic!("unauthorized handler ran");
}

#[tokio::test]
async fn browser_requests_are_denied_and_response_data_retains_admission() {
    let access = Access {
        token: "a".repeat(64),
        requests: Arc::new(Semaphore::new(8)),
    };
    let app = Router::new()
        .route("/", post(|| async { "ok" }))
        .layer(middleware::from_fn_with_state(access, http::protect))
        .layer(middleware::from_fn(board_http::retain_response_body));
    let request =
        || Request::post("/").header("authorization", format!("Bearer {}", "a".repeat(64)));
    for name in ["origin", "sec-fetch-site"] {
        let response = app
            .clone()
            .oneshot(request().header(name, "null").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let mut bodies = Vec::new();
    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(request().body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        bodies.push(to_bytes(response.into_body(), 10).await.unwrap());
    }
    let response = app
        .clone()
        .oneshot(request().body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "1");
    bodies.clear();
    assert_eq!(
        app.oneshot(request().body(Body::empty()).unwrap())
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
