#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn response(app: &Router, path: &str) -> (StatusCode, Value, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let etag = response
        .headers()
        .get("etag")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    let body = if status == StatusCode::OK {
        serde_json::from_slice(&bytes).unwrap()
    } else {
        Value::Null
    };
    (status, body, etag)
}

async fn upload_preflight(app: &Router, board: &str, parent: i64) -> StatusCode {
    // A finite multipart form stops just after the parent field. Policy runs
    // before the next-field check or any network upload reservation. A 422
    // therefore proves that the parent was admitted by the live preflight.
    let body = format!(
        "--retention\r\nContent-Disposition: form-data; name=\"resto\"\r\n\r\n{parent}\r\n--retention--\r\n"
    );
    app.clone()
        .oneshot(
            Request::post(format!("/{board}/upload"))
                .header("origin", ORIGIN)
                .header("content-type", "multipart/form-data; boundary=retention")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn sticky_window_admits_upload_preflight_and_publishes_surviving_json_on_both_listeners() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'sr'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES ($1,'Owned source sticky HTTP','Synthetic',2000,3,3,100,10,10,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    let post = board_store::NewPost {
        name: "Anonymous".into(),
        subject: "Owned sticky HTTP".into(),
        comment: "Synthetic source preview response".into(),
        deletion_hash: "synthetic-password-proof".into(),
        sage: false,
    };
    let op = posting::create_post(&public, &board, 0, &post)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET sticky=true,undead=true WHERE board=$1 AND id=$2")
        .bind(&board)
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(
            posting::create_post(&public, &board, op, &post)
                .await
                .unwrap(),
        );
    }
    let media = board_config::PublicMediaSettings::development(
        "127.0.0.1:1",
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "1000".into())
    })
    .unwrap();
    let (web, api) = posting::routers_with_limits(
        public.clone(),
        &board,
        ORIGIN.into(),
        false,
        Some(media),
        limits,
    );
    let path = format!("/{board}/thread/{op}.json");
    let before = response(&web, &path).await;
    assert_eq!(before.0, StatusCode::OK);
    assert_eq!(before.1["posts"].as_array().unwrap().len(), 4);
    assert_eq!(
        upload_preflight(&web, &board, op).await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let next = posting::create_post(&public, &board, op, &post)
        .await
        .unwrap();
    ids.remove(0);
    ids.push(next);
    for router in [&web, &api] {
        let (status, json, tag) = response(router, &path).await;
        assert_eq!(status, StatusCode::OK);
        assert_ne!(tag, before.2);
        assert_eq!(json["posts"][0]["replies"], 3);
        let actual: Vec<i64> = json["posts"].as_array().unwrap()[1..]
            .iter()
            .map(|post| post["no"].as_i64().unwrap())
            .collect();
        assert_eq!(actual, ids);
        for resource in [format!("/{board}/1.json"), format!("/{board}/catalog.json")] {
            let (status, value, _) = response(router, &resource).await;
            assert_eq!(status, StatusCode::OK, "{resource}");
            let source = if resource.ends_with("catalog.json") {
                &value[0]["threads"][0]
            } else {
                &value["threads"][0]["posts"][0]
            };
            assert_eq!(source["replies"], 3);
            assert_eq!(source["no"], op);
        }
    }
    // Neither a normal full thread nor a closed thread acquires this special
    // preflight. The same board and OP make the comparison meaningful.
    sqlx::query("UPDATE content.threads SET undead=false WHERE board=$1 AND id=$2")
        .bind(&board)
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        upload_preflight(&web, &board, op).await,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE content.threads SET undead=true,closed=true WHERE board=$1 AND id=$2")
        .bind(&board)
        .bind(op)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        upload_preflight(&web, &board, op).await,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE content.threads SET closed=false,sticky=false,undead=false WHERE board=$1 AND id=$2")
        .bind(&board).bind(op).execute(&owner).await.unwrap();
    posting::cleanup_posting(&owner, &board).await;
    for sql in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(sql).bind(&board).execute(&owner).await.unwrap();
    }
    public.close().await;
    owner.close().await;
}
