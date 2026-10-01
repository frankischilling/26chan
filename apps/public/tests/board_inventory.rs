#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

#[tokio::test]
async fn installed_inventory_policy_routes_and_private_content_match_the_reference() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reference: Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let boards = reference["boards"].as_array().unwrap();
    assert_eq!(boards.iter().filter(|b| b["listed"] == true).count(), 80);
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    for expected in boards {
        let slug = expected["slug"].as_str().unwrap();
        let saved: Value =
            sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug=$1")
                .bind(slug)
                .fetch_one(&owner)
                .await
                .unwrap();
        for (key, value) in expected.as_object().unwrap() {
            if !matches!(key.as_str(), "listed" | "source_policy") {
                assert_eq!(&saved[key], value, "/{slug}/: {key}");
            }
        }
        let private = expected["staff_only"] == true;
        let rss_enabled = expected["source_policy"]["USE_RSS"] == "yes";
        assert_eq!(saved["rss_enabled"], rss_enabled, "/{slug}/: RSS policy");
        assert_eq!(
            saved["robot9000"],
            expected["source_policy"]["ROBOT9000"] == "yes",
            "/{slug}/: Robot9000 policy"
        );
        if !private {
            board_store::board_page_snapshot(
                &public,
                slug,
                board_store::BoardSelection::Page(1),
                Some(3),
            )
            .await
            .unwrap();
        }
        for (router, path, available) in [
            (&app, format!("/{slug}/"), !private),
            (&app, format!("/{slug}/index.rss"), !private && rss_enabled),
            (
                &app,
                format!("/{slug}/catalog"),
                !private && expected["catalog_enabled"] == true,
            ),
            (
                &app,
                format!("/{slug}/archive"),
                !private && expected["archive_retention_seconds"].as_i64().unwrap() > 0,
            ),
            (
                &api,
                format!("/{slug}/1.json"),
                !private && expected["json_enabled"] == true,
            ),
            (
                &api,
                format!("/{slug}/catalog.json"),
                !private && expected["json_enabled"] == true && expected["catalog_enabled"] == true,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(&path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body = to_bytes(response.into_body(), 3_000_000).await.unwrap();
            assert_eq!(
                status,
                if available { 200 } else { 404 },
                "{path}: {}",
                String::from_utf8_lossy(&body)
            );
        }
    }
    let directory = board_store::boards(&public).await.unwrap();
    assert!(!directory.iter().any(|b| b.slug == "j"));
    assert_eq!(directory.iter().filter(|b| b.source_order < 80).count(), 79);
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let html = String::from_utf8(
        to_bytes(response.into_body(), 200_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    for board in &directory {
        assert!(
            html.contains(&format!("href=\"/{}/\"", board.slug)),
            "Missing /{}/",
            board.slug
        );
    }
    assert!(!html.contains("href=\"/j/\""));

    // Healthy private rows exist in the allowed context. A negative public
    // SELECT therefore proves a boundary rather than an empty destination.
    let mut tx = owner.begin().await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES('j') RETURNING id")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,'j',$1,'Anonymous','','Owned private fixture')").bind(id).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'owned private fixture')").bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for query in [
        "SELECT count(*) FROM content.threads WHERE id=$1",
        "SELECT count(*) FROM content.posts WHERE id=$1",
        "SELECT count(*) FROM content.visible_threads WHERE id=$1",
    ] {
        let count: i64 = sqlx::query_scalar(query)
            .bind(id)
            .fetch_one(&public)
            .await
            .unwrap();
        assert_eq!(count, 0, "{query}");
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion WHERE post_id=$1")
            .bind(id)
            .fetch_one(&public)
            .await
            .unwrap();
    assert_eq!(count, 0);
    assert!(
        sqlx::query("INSERT INTO content.threads(board) VALUES('j')")
            .execute(&public)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
}
