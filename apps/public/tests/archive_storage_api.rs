#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str, etag: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(path);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn complete_archive_ids_and_html_window_remain_distinct_on_both_listeners() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'aj'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES($1,'Archive API','Owned synthetic',2000,1000,300,1,1,864000)").bind(&board).execute(&owner).await.unwrap();
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let mut ids: Vec<i64> = sqlx::query_scalar("WITH inserted AS (INSERT INTO content.threads(board,archived_at,archive_expires_at,bumped_at) SELECT $1,statement_timestamp()-interval '4 days',statement_timestamp()+interval '5 days',CASE WHEN n=1 THEN statement_timestamp()-interval '73 hours' ELSE statement_timestamp() END FROM generate_series(1,3002) n RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Anonymous','Owned archive','Synthetic archive API' FROM inserted RETURNING id").bind(&b).fetch_all(&o).await.unwrap();
        ids.sort_unstable();
        let (web, api) = board_public::routers_with_limits(p.clone(), "http://127.0.0.1:3000".into(), false, None, board_config::PublicRequestLimits::default());
        let path = format!("/{b}/archive.json");
        for app in [&web, &api] {
            let response = get(app, &path, None).await;
            assert_eq!(response.status(), StatusCode::OK);
            let etag = response.headers()["etag"].to_str().unwrap().to_owned();
            let json: Value = serde_json::from_slice(&to_bytes(response.into_body(), 4*1024*1024).await.unwrap()).unwrap();
            assert_eq!(json, serde_json::json!(ids));
            assert_eq!(get(app, &path, Some(&etag)).await.status(), StatusCode::NOT_MODIFIED);
            sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
            assert_eq!(get(app, &path, Some(&etag)).await.status(), StatusCode::NOT_FOUND,
                "a matching ETag cannot bypass disabled archive visibility");
            sqlx::query("UPDATE content.boards SET archive_retention_seconds=864000 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        }
        let html = get(&web, &format!("/{b}/archive"), None).await;
        assert_eq!(html.status(), StatusCode::OK);
        let html = String::from_utf8(to_bytes(html.into_body(), 8*1024*1024).await.unwrap().to_vec()).unwrap();
        assert_eq!(html.matches("class=\"quotelink\"").count(), 3000);
        assert!(!html.contains(&format!("href=\"/{b}/thread/{}\"", ids[0])));
        let limits = board_config::PublicRequestLimits::from_lookup(|key| (key == "PUBLIC_MAX_RESPONSE_BYTES").then(|| "64".into())).unwrap();
        let (small_web, small_api) = board_public::routers_with_limits(p, "http://127.0.0.1:3000".into(), false, None, limits);
        for app in [&small_web, &small_api] {
            assert_eq!(get(app, &path, None).await.status(), StatusCode::SERVICE_UNAVAILABLE,
                "response budgets reject the whole list instead of truncating IDs");
        }
    }).await;
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}

#[tokio::test]
async fn current_age_and_unlimited_policy_control_both_archive_listeners() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'au'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES($1,'Archive age','Owned synthetic',2000,1000,300,1,1,3600)")
        .bind(&board).execute(&owner).await.unwrap();
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let id: i64 = sqlx::query_scalar("WITH inserted AS (INSERT INTO content.threads(board,archived_at,archive_expires_at) VALUES($1,statement_timestamp()-interval '2 hours',statement_timestamp()-interval '1 hour') RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Anonymous','Retained archive','Synthetic archive age' FROM inserted RETURNING id")
            .bind(&b).fetch_one(&o).await.unwrap();
        let (web, api) = board_public::routers_with_limits(p, "http://127.0.0.1:3000".into(), false, None, board_config::PublicRequestLimits::default());
        let path = format!("/{b}/archive.json");
        for policy in [3600, -1, 0, 10800, 3600] {
            sqlx::query("UPDATE content.boards SET archive_retention_seconds=$2 WHERE slug=$1")
                .bind(&b).bind(policy).execute(&o).await.unwrap();
            for app in [&web, &api] {
                let response = get(app, &path, None).await;
                if policy == 0 {
                    assert_eq!(response.status(), StatusCode::NOT_FOUND);
                    continue;
                }
                assert_eq!(response.status(), StatusCode::OK);
                let etag = response.headers()["etag"].to_str().unwrap().to_owned();
                let json: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap()).unwrap();
                let expected = if policy == 3600 { serde_json::json!([]) } else { serde_json::json!([id]) };
                assert_eq!(json, expected);
                assert_eq!(get(app, &path, Some(&etag)).await.status(), StatusCode::NOT_MODIFIED);
                let thread = get(app, &format!("/{b}/thread/{id}.json"), None).await;
                assert_eq!(thread.status(), if policy == 3600 { StatusCode::NOT_FOUND } else { StatusCode::OK });
            }
            if policy == -1 {
                let response = get(&web, &format!("/{b}/archive"), None).await;
                assert_eq!(response.status(), StatusCode::OK);
                let html = String::from_utf8(to_bytes(response.into_body(), 1024*1024).await.unwrap().to_vec()).unwrap();
                assert!(html.contains(&format!("/{b}/thread/{id}")));
            }
        }
    }).await;
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
