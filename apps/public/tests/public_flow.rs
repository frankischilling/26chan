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
    let app = fixture_routers(
        pool.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        None,
        board_config::PublicRequestLimits::default(),
    )
    .0;
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/fixture/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let request = Request::builder().method("POST").uri("/fixture/post")
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
                .uri(format!("/fixture/thread/{thread}.json"))
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
                .uri(format!("/fixture/thread/{thread}.json"))
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
            .uri("/fixture/delete")
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
                    .uri("/fixture/delete")
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
                .uri(format!("/fixture/thread/{thread}.json"))
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

// Fixture transport identities are isolated so unrelated authorization cases do
// not consume each other's shared public deletion quota.
fn fixture_peer() -> std::net::SocketAddr {
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();
    let nonce = NEXT
        .get_or_init(|| {
            std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
            )
        })
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::net::SocketAddr::new(
        std::net::IpAddr::V6(std::net::Ipv6Addr::new(
            0x2001,
            0xdb8,
            5,
            0,
            (nonce >> 48) as u16,
            (nonce >> 32) as u16,
            (nonce >> 16) as u16,
            nonce as u16,
        )),
        12345,
    )
}

fn fixture_routers(
    pool: sqlx::PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (axum::Router, axum::Router) {
    let (web, api) = board_public::routers_with_options(
        pool,
        board_public::PublicRouterOptions {
            origin,
            production,
            media,
            limits,
            proxy_uid: None,
            poster_id_key: Some(std::sync::Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&"42".repeat(32)).unwrap(),
            )),
            tripcode_key: None,
            country_database: None,
        },
    );

    let transport = axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .is_none()
            {
                request
                    .extensions_mut()
                    .insert(axum::extract::ConnectInfo(fixture_peer()));
            }
            next.run(request).await
        },
    );
    (web.layer(transport.clone()), api.layer(transport))
}
