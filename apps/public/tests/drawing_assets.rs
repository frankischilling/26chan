use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn directive<'a>(policy: &'a str, name: &str) -> Option<&'a str> {
    policy
        .split(';')
        .map(str::trim)
        .find(|value| value.split_ascii_whitespace().next() == Some(name))
}

#[tokio::test]
async fn tegaki_routes_serve_only_pinned_release_bytes_without_execution_authority() {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/tegaki-assets-reference.json"
    ))
    .unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
        .unwrap();
    for origin in ["http://127.0.0.1:3000", "https://board.example"] {
        for (path, mime, expected) in [
            (
                "/static/tegaki/tegaki-0.9.4.v1.js",
                "text/javascript; charset=utf-8",
                include_bytes!("../static/tegaki/tegaki-0.9.4.v1.js").as_slice(),
            ),
            (
                "/static/tegaki/tegaki-0.9.4.v1.css",
                "text/css; charset=utf-8",
                include_bytes!("../static/tegaki/tegaki-0.9.4.v1.css").as_slice(),
            ),
            (
                "/static/tegaki/tegaki-icons.v1.woff",
                "font/woff",
                include_bytes!("../static/tegaki/tegaki-icons.v1.woff").as_slice(),
            ),
        ] {
            let pinned = manifest["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["release_path"] == path)
                .unwrap();
            let (web, api) =
                board_public::routers(pool.clone(), origin.into(), origin.starts_with("https:"));
            for method in ["GET", "HEAD", "POST", "PUT", "DELETE"] {
                let response = web
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
                if !matches!(method, "GET" | "HEAD") {
                    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
                    continue;
                }
                assert_eq!(response.status(), StatusCode::OK, "{method} {path}");
                assert_eq!(response.headers()["content-type"], mime);
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
                assert_eq!(
                    response.headers()["cache-control"],
                    "public, max-age=0, must-revalidate"
                );
                assert!(response.headers().get("set-cookie").is_none());
                let policy = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                for name in ["default-src", "script-src", "connect-src", "worker-src"] {
                    assert_eq!(
                        directive(policy, name),
                        Some(format!("{name} 'none'").as_str())
                    );
                }
                assert!(directive(policy, "font-src").is_none());
                assert!(
                    !policy.contains("tegaki")
                        && !policy.contains("blob:")
                        && !policy.contains("data:")
                );
                if let Some(images) = directive(policy, "img-src") {
                    assert!(
                        !images
                            .split_ascii_whitespace()
                            .any(|value| value == "'self'" || value == "*")
                    );
                }
                let body = to_bytes(response.into_body(), 1_048_576).await.unwrap();
                if method == "HEAD" {
                    assert!(body.is_empty());
                } else {
                    assert_eq!(body.as_ref(), expected);
                    assert_eq!(body.len() as u64, pinned["bytes"].as_u64().unwrap());
                    assert_eq!(
                        format!("{:x}", Sha256::digest(&body)),
                        pinned["sha256"].as_str().unwrap()
                    );
                }
            }
            let response = api
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        let web = board_public::router(pool.clone(), origin.into(), origin.starts_with("https:"));
        for path in [
            "/static/tegaki/tegaki.min.js",
            "/static/tegaki/tegaki-0.9.4.v2.js",
            "/static/tegaki/tegaki-0.9.4.v2.css",
            "/static/tegaki/tegaki-icons.v2.woff",
            "/static/tegaki/fontello-config.json",
            "/static/tegaki/unknown.png",
            "/static/tegaki/../secret.js",
            "/static/tegaki/tegaki-0.9.4.v1.js/extra",
        ] {
            let response = web
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            let policy = response.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            assert!(!policy.contains("tegaki") && !policy.contains("blob:"));
        }
    }
}
