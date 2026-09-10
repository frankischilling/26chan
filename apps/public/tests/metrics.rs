use axum::{Router, body::Body, http::Request};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

async fn send(app: &Router, method: &str, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn metrics_observe_final_responses_without_releasing_shared_body_admission() {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    let (metrics, public, api) =
        board_public::observed_routers(pool, "http://localhost:3000".into(), false, true);
    let mut held = Vec::new();
    for app in [&public, &api] {
        for _ in 0..16 {
            let response = send(app, "GET", "/healthz").await;
            assert_eq!(response.status(), 200);
            held.push(response);
        }
    }
    let saturated = api
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header("origin", "http://localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(saturated.status(), 503);
    assert_eq!(
        saturated.headers()["access-control-allow-origin"],
        "http://localhost:3000"
    );
    assert!(
        metrics
            .render()
            .contains("board_http_responses_total{listener=\"api\",status_class=\"5xx\"} 1\n")
    );
    assert!(
        metrics
            .render()
            .contains("board_http_handlers_inflight{listener=\"public\"} 0\n")
    );
    drop(held.pop());
    assert_eq!(send(&public, "HEAD", "/healthz").await.status(), 200);
    drop(held);
    assert_eq!(
        send(&public, "POST", "/b/post?secret=synthetic-secret")
            .await
            .status(),
        403
    );
    assert_eq!(send(&api, "GET", "/metrics").await.status(), 404);
    let missing = send(&public, "GET", "/synthetic-secret/absent/path/long").await;
    assert_eq!(missing.status(), 404);
    assert_eq!(missing.headers()["x-content-type-options"], "nosniff");
    let snapshot = metrics.render();
    assert!(snapshot.contains("board_http_write_rejections_total{listener=\"public\"} 1\n"));
    assert!(
        snapshot.contains("board_http_authorization_rejections_total{listener=\"public\"} 1\n")
    );
    assert!(snapshot.contains("board_db_pool_max_connections{pool=\"public\"} 12\n"));
    assert!(!snapshot.contains("synthetic-secret"));
    assert!(!snapshot.contains("postgres"));
}

#[tokio::test]
async fn unserved_api_listener_has_no_metric_series() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    let (metrics, public, _unused_api) =
        board_public::observed_routers(pool, "http://localhost:3000".into(), false, false);
    assert_eq!(send(&public, "GET", "/healthz").await.status(), 200);
    assert!(metrics.render().contains("listener=\"public\""));
    assert!(!metrics.render().contains("listener=\"api\""));
}
