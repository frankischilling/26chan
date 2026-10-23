#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::sync::Arc;
use tower::ServiceExt;
#[tokio::test]
async fn event_migrations_preserve_all_public_readiness_gates() {
    let pool = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for function in [
        "content.lock_report_network_policy()",
        "content.network_admission_rules(text,bigint)",
    ] {
        assert!(
            !sqlx::query_scalar::<_, bool>(
                "SELECT has_function_privilege(current_user,$1,'EXECUTE')"
            )
            .bind(function)
            .fetch_one(&mut *tx)
            .await
            .unwrap()
        );
    }
    let denied = sqlx::query("SELECT actor_hash FROM post_secrets.posting_history LIMIT 0")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    owner.close().await;
    for enabled in [false, true] {
        let web = board_public::routers_with_events(
            pool.clone(),
            board_public::PublicRouterOptions {
                origin: "http://localhost:3000".into(),
                production: false,
                media: None,
                limits: Default::default(),
                proxy_uid: None,
                poster_id_key: Some(Arc::new(
                    board_domain::poster_id::PosterIdKey::parse(&"44".repeat(32)).unwrap(),
                )),
                tripcode_key: None,
                country_database: None,
            },
            None,
            Default::default(),
            enabled,
        )
        .0;
        let response = web
            .oneshot(Request::get("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 32768).await.unwrap();
        assert_eq!(
            status,
            200,
            "events={enabled}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    pool.close().await;
}
