#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use tower::ServiceExt;

async fn read(
    app: &Router,
    method: &str,
    path: &str,
    tag: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://127.0.0.1:3000");
    if let Some(tag) = tag {
        request = request
            .header("if-none-match", tag)
            .header("if-modified-since", "Mon, 01 Jan 2040 00:00:00 GMT");
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn stats(app: &Router, path: &str) -> (serde_json::Value, String) {
    let (status, headers, body) = read(app, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/json");
    assert!(!headers.contains_key("last-modified"));
    assert!(!headers.contains_key("set-cookie"));
    assert!(body.len() <= 1024);
    (
        serde_json::from_slice(&body).unwrap(),
        headers["etag"].to_str().unwrap().into(),
    )
}

#[tokio::test]
async fn stats_preserve_exact_ids_current_counts_page_validators_and_transaction_visibility() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0; 4];
    OsRng.fill_bytes(&mut random);
    let suffix = u32::from_le_bytes(random);
    let slug = format!("st{suffix:08x}");
    // Explicit fixture IDs exercise exact values beyond JavaScript's safe integer
    // range without changing the database's ordinary post-number sequence.
    let id = (1i64 << 54) + i64::from(suffix) * 100;
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,archive_retention_seconds,archive_limit) VALUES($1,'Owned stats','Synthetic coherent counts',4000,1000,2,10,1,0,3600,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let fixture = slug.clone();
    let admin = owner.clone();
    let result = tokio::spawn(async move {
        for (thread, age) in [(id, 60), (id + 10, 30)] {
            sqlx::query("INSERT INTO content.threads(id,board,bumped_at) VALUES($1,$2,clock_timestamp()-make_interval(secs=>$3))")
                .bind(thread).bind(&fixture).bind(f64::from(age)).execute(&admin).await.unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Owned opening post')")
                .bind(thread).bind(&fixture).execute(&admin).await.unwrap();
        }
        for offset in 1..=3 {
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,deleted) VALUES($1,$2,$3,'Anonymous','','Owned reply',$4)")
                .bind(id + offset).bind(&fixture).bind(id).bind(offset == 2).execute(&admin).await.unwrap();
        }
        let (app, api) = board_public::routers(public, "http://127.0.0.1:3000".into(), false);
        let path = format!("/_watch/{fixture}/thread/{id}/stats");
        let (initial, tag) = stats(&app, &path).await;
        assert_eq!(initial, serde_json::json!({"version":1,"board":fixture,"thread":id.to_string(),
            "replies":2,"images":0,"sticky":false,"closed":false,"archived":false,
            "bump_limited":true,"image_limited":true,"page":2}));
        assert_eq!(read(&api, "GET", &path, None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(read(&app, "POST", &path, None).await.0, StatusCode::METHOD_NOT_ALLOWED);
        let head = read(&app, "HEAD", &path, None).await;
        assert_eq!(head.0, StatusCode::OK); assert!(head.2.is_empty()); assert_eq!(head.1["etag"], tag);
        let cached = read(&app, "GET", &path, Some(&tag)).await;
        assert_eq!(cached.0, StatusCode::NOT_MODIFIED); assert!(cached.2.is_empty());
        assert_eq!(read(&app, "GET", &format!("/_watch/missing/thread/{id}/stats"), None).await.0, StatusCode::NOT_FOUND);

        let modified: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT http_modified_at FROM content.threads WHERE id=$1").bind(id).fetch_one(&admin).await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(id + 10).execute(&admin).await.unwrap();
        let changed = read(&app, "GET", &path, Some(&tag)).await;
        assert_eq!(changed.0, StatusCode::OK);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&changed.2).unwrap()["page"], 1);
        assert_ne!(changed.1["etag"], tag);
        assert_eq!(sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>("SELECT http_modified_at FROM content.threads WHERE id=$1").bind(id).fetch_one(&admin).await.unwrap(), modified);

        let mut transaction = admin.begin().await.unwrap();
        sqlx::query("UPDATE content.threads SET sticky=true,closed=true WHERE id=$1").bind(id).execute(&mut *transaction).await.unwrap();
        assert_eq!(stats(&app, &path).await.0["sticky"], false);
        transaction.commit().await.unwrap();
        let flagged = stats(&app, &path).await.0;
        assert_eq!(flagged["sticky"], true); assert_eq!(flagged["closed"], true);
        assert_eq!(flagged["bump_limited"], false); assert_eq!(flagged["image_limited"], false);
        sqlx::query("UPDATE content.threads SET sticky=false,undead=true WHERE id=$1").bind(id).execute(&admin).await.unwrap();
        let undead = stats(&app, &path).await.0;
        assert_eq!(undead["bump_limited"], true); assert_eq!(undead["image_limited"], false);
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(id + 1).execute(&admin).await.unwrap();
        assert_eq!(stats(&app, &path).await.0["replies"], 1);
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(id).execute(&admin).await.unwrap();
        let archived = stats(&app, &path).await.0;
        assert_eq!(archived["archived"], true); assert!(archived["page"].is_null());
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 seconds',archive_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(id).execute(&admin).await.unwrap();
        assert_eq!(read(&app, "GET", &path, None).await.0, StatusCode::NOT_FOUND);
    }).await;
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
