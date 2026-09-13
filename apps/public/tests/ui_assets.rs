use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

#[tokio::test]
async fn catalog_script_is_release_owned_and_not_an_image_source() {
    let origin = "http://127.0.0.1:3000";
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
        .unwrap();
    let app = board_public::router(pool, origin.into(), false);
    for method in ["GET", "HEAD", "POST", "PUT", "DELETE"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/static/catalog-preferences.v1.js")
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
            assert_eq!(
                response.headers()["cache-control"],
                "public, max-age=0, must-revalidate"
            );
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            assert!(response.headers().get("set-cookie").is_none());
            let csp = response.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            assert!(csp.contains("script-src 'none'"));
            assert!(!csp.contains("catalog-preferences"));
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            if method == "GET" {
                assert_eq!(
                    bytes.as_ref(),
                    include_bytes!("../static/catalog-preferences.v1.js")
                );
            } else {
                assert!(bytes.is_empty());
            }
        } else {
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
    }
    for path in [
        "/static/catalog-preferences.js",
        "/static/catalog-preferences.v2.js",
        "/static/secret.js",
    ] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn catalog_assets_are_fixed_bytes_with_narrow_csp_and_no_write_route() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/public-catalog-assets.json")).unwrap();
    for origin in ["http://127.0.0.1:3000", "https://board.example"] {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
            .unwrap();
        let app = board_public::router(pool, origin.into(), origin.starts_with("https:"));
        let mut sources =
            format!("img-src {origin}/static/themes/fade.png {origin}/static/themes/fade-blue.png");
        for asset in manifest["assets"].as_array().unwrap() {
            sources.push_str(&format!(
                " {origin}/static/catalog/{}",
                asset["name"].as_str().unwrap()
            ));
        }
        sources.push(';');
        for asset in manifest["assets"].as_array().unwrap() {
            let path = format!("/static/catalog/{}", asset["name"].as_str().unwrap());
            for method in ["GET", "HEAD"] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(&path)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response.headers()["content-type"],
                    asset["mime"].as_str().unwrap()
                );
                assert_eq!(
                    response.headers()["cache-control"],
                    "public, max-age=0, must-revalidate"
                );
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
                assert!(response.headers().get("set-cookie").is_none());
                let csp = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                assert!(csp.contains(&sources));
                assert!(!csp.contains("img-src 'self'"));
                assert!(csp.contains("script-src 'none'"));
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                if method == "HEAD" {
                    assert!(bytes.is_empty());
                } else {
                    assert_eq!(bytes.len() as u64, asset["bytes"].as_u64().unwrap());
                    assert_eq!(
                        format!("{:x}", Sha256::digest(bytes)),
                        asset["sha256"].as_str().unwrap()
                    );
                }
            }
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&path)
                        .header("origin", origin)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
        for path in [
            "/static/catalog/unknown.png",
            "/static/catalog/spoiler-other.png",
            "/static/catalog/../secret",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }
}
