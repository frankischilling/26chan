#![cfg(feature = "database-tests")]
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Requires an isolated migrated database. Never silently skips absent services.
#[tokio::test]
async fn persisted_posting_json_deletion_and_database_denials() {
    let database =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&database).await.unwrap();
    let app = board_public::router(pool.clone(), "http://127.0.0.1:3000".into(), false);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/test/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let request = Request::builder().method("POST").uri("/test/post")
        .header("origin", "http://127.0.0.1:3000").header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("name=Tester&sub=Persistence&com=%3Cscript%3Ealert%281%29%3C%2Fscript%3E&password=test-password-123&resto=0")).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let thread = location
        .split('/')
        .next_back()
        .unwrap()
        .split('#')
        .next()
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/test/thread/{thread}.json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()["etag"].clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["posts"][0]["no"], thread.parse::<i64>().unwrap());
    assert_eq!(value["posts"][0]["resto"], 0);
    assert_eq!(value["posts"][0]["replies"], 0);
    assert!(
        value["posts"][0]["com"]
            .as_str()
            .unwrap()
            .contains("&#60;script&#62;")
    );
    assert!(value["posts"][0].get("password").is_none());
    assert!(value["posts"][0].get("tim").is_none());
    let cached = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/test/thread/{thread}.json"))
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);

    for origin in [None, Some("https://untrusted.example"), Some("null")] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/test/delete")
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from(format!(
                        "no={thread}&password=test-password-123"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for password in ["wrong-password", "test-password-123"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/test/delete")
                    .header("origin", "http://127.0.0.1:3000")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!("no={thread}&password={password}")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if password == "wrong-password" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::SEE_OTHER
            }
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/test/thread/{thread}.json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    for query in [
        "SELECT * FROM staff_identity.credentials",
        "UPDATE staff_identity.accounts SET role = 'admin'",
        "CREATE SCHEMA forbidden",
        "SELECT * FROM deployment.settings",
        "SET ROLE board_migrator",
        "SELECT * FROM content.reports",
    ] {
        let error = sqlx::query(query).execute(&pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501"),
            "expected an actual permission denial: {query}"
        );
    }
    let migration_url = std::env::var("MIGRATION_DATABASE_URL")
        .expect("MIGRATION_DATABASE_URL is required for positive permission controls");
    let migrator = sqlx::PgPool::connect(&migration_url).await.unwrap();
    for query in [
        "SELECT * FROM staff_identity.credentials",
        "SELECT * FROM deployment.settings",
        "SELECT * FROM content.reports",
    ] {
        sqlx::query(query).execute(&migrator).await.unwrap();
    }
    assert!(matches!(
        board_store::connect_public(&migration_url).await,
        Err(board_store::StoreError::UnsafeRole)
    ));
    migrator.close().await;
    pool.close().await;
}
