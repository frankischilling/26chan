use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn math_resources_are_exact_release_bytes_and_networkless_workers() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
        .unwrap();
    let origin = "http://127.0.0.1:3000";
    let app = board_public::router(pool, origin.into(), false);
    for (path, expected) in [
        (
            "/static/native-math.v1.js",
            include_bytes!("../static/native-math.v1.js").as_slice(),
        ),
        (
            "/static/native-math-worker.v1.js",
            include_bytes!("../static/native-math-worker.v1.js").as_slice(),
        ),
    ] {
        for method in ["GET", "HEAD", "POST", "PUT", "DELETE"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if matches!(method, "GET" | "HEAD") {
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response.headers()["content-type"],
                    "text/javascript; charset=utf-8"
                );
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
                assert_eq!(
                    response.headers()["cache-control"],
                    "public, max-age=0, must-revalidate"
                );
                let policy = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                assert_eq!(
                    policy,
                    "default-src 'none'; script-src 'none'; connect-src 'none'; worker-src 'none'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'"
                );
                assert!(response.headers().get("set-cookie").is_none());
                let body = to_bytes(response.into_body(), 16_777_216).await.unwrap();
                if method == "GET" {
                    assert_eq!(body.as_ref(), expected);
                } else {
                    assert!(body.is_empty());
                }
            } else {
                assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
            }
        }
    }
    for path in [
        "/static/native-math.js",
        "/static/native-math.v2.js",
        "/static/native-math-worker.v2.js",
        "/static/mathjax/MathJax.js",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            !response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("native-math")
        );
    }
}

#[test]
fn all_source_boards_resolve_math_policy_and_default_is_disabled() {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let boards = reference["boards"].as_array().unwrap();
    assert_eq!(boards.len(), 82);
    for board in boards {
        assert_eq!(board["math_tags"], board["slug"] == "sci");
        assert_eq!(
            board["math_tags"],
            board["source_policy"]["JSMATH"] == "yes"
        );
    }
    let migration = include_str!("../../../migrations/0113_board_math_display.sql");
    assert!(migration.contains("math_tags boolean NOT NULL DEFAULT false"));
    assert!(migration.contains("SET math_tags=true WHERE slug='sci'"));
}
