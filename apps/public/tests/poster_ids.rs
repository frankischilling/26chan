#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn options(key: bool) -> board_public::PublicRouterOptions {
    board_public::PublicRouterOptions {
        country_database: None,
        origin: "https://boards.example.com".into(),
        production: true,
        media: None,
        limits: board_config::PublicRequestLimits::default(),
        proxy_uid: None,
        tripcode_key: None,
        poster_id_key: key.then(|| {
            Arc::new(board_domain::poster_id::PosterIdKey::parse(&"1".repeat(64)).unwrap())
        }),
    }
}
async fn post(app: &Router, board: &str, parent: i64, peer: Option<&str>) -> Value {
    let mut request = Request::post(format!("/{board}/post"))
        .header("origin", "https://boards.example.com")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .header("x-forwarded-for", "198.51.100.200")
        .header("x-board-client-ip", "198.51.100.201")
        .body(Body::from(format!("resto={parent}&name=Owned&sub=Owned&com=Owned+poster+identity&pwd=owned-poster-password"))).unwrap();
    if let Some(peer) = peer {
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<SocketAddr>().unwrap(),
        ));
    }
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), if peer.is_none() { 503 } else { 200 });
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}
async fn get(app: &Router, path: &str) -> (String, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    let etag = response
        .headers()
        .get("etag")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), 4_194_304).await.unwrap();
    (etag, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn poster_labels_are_persisted_scoped_and_never_supplied_by_headers_or_fields() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,user_ids) VALUES($1,'Owned poster IDs','Synthetic fixture',1000,100,100,100,10,true)").bind(&board).execute(&owner).await.unwrap();
    let (app, api) = board_public::routers_with_options(public.clone(), options(true));
    let no_key = board_public::routers_with_options(public.clone(), options(false)).0;
    assert_eq!(
        post(&no_key, &board, 0, Some("192.0.2.10:9000")).await["error"],
        "Poster IDs are unavailable."
    );
    assert_eq!(
        post(&app, &board, 0, None).await["error"],
        "Posting transport identity is unavailable."
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.threads WHERE board=$1")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap(),
        0
    );
    let first = post(&app, &board, 0, Some("192.0.2.10:9000")).await;
    let thread = first["pid"].as_i64().unwrap();
    let original = board_store::find_post(&public, &board, thread)
        .await
        .unwrap()
        .poster_id
        .unwrap();
    assert_eq!(
        original,
        board_domain::poster_id::PosterIdKey::parse(&"1".repeat(64))
            .unwrap()
            .label(&board, thread, "192.0.2.10".parse().unwrap())
            .unwrap()
    );
    let (etag, value) = get(&api, &format!("/{board}/thread/{thread}.json")).await;
    assert_eq!(value["posts"][0]["id"], original);
    let (_, public_json) = get(&app, &format!("/{board}/thread/{thread}.json")).await;
    assert_eq!(public_json, value);
    let (_, boards) = get(&api, "/boards.json").await;
    assert_eq!(
        boards["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["board"] == board)
            .unwrap()["user_ids"],
        1
    );
    for peer in ["192.0.2.10:9001", "[::ffff:192.0.2.10]:9002"] {
        let value = post(&app, &board, thread, Some(peer)).await;
        let id = value["pid"].as_i64().unwrap();
        assert_eq!(
            board_store::find_post(&public, &board, id)
                .await
                .unwrap()
                .poster_id
                .as_deref(),
            Some(original.as_str())
        );
    }
    let other = post(&app, &board, thread, Some("192.0.2.11:9000")).await["pid"]
        .as_i64()
        .unwrap();
    assert_ne!(
        board_store::find_post(&public, &board, other)
            .await
            .unwrap()
            .poster_id
            .as_deref(),
        Some(original.as_str())
    );
    let second_thread = post(&app, &board, 0, Some("192.0.2.10:9000")).await["pid"]
        .as_i64()
        .unwrap();
    assert_ne!(
        board_store::find_post(&public, &board, second_thread)
            .await
            .unwrap()
            .poster_id
            .as_deref(),
        Some(original.as_str())
    );
    let (changed, value) = get(&api, &format!("/{board}/thread/{thread}.json")).await;
    assert_ne!(etag, changed);
    assert_eq!(value["posts"][1]["id"], original);
    for path in [format!("/{board}/"), format!("/{board}/thread/{thread}")] {
        let response = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let bytes = to_bytes(response.into_body(), 4_194_304).await.unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains(&format!(
            "<span class=\"posteruid\">(ID: <span class=\"hand\">{original}</span>)</span>"
        )));
        assert!(!html.contains("192.0.2.10") && !html.contains("FORGEDID"));
    }
    let (_, preview) = get(&app, &format!("/_watch/{board}/post/{other}")).await;
    assert!(preview.to_string().contains("posteruid"));
    for path in [format!("/{board}/catalog.json"), format!("/{board}/1.json")] {
        let (_, value) = get(&api, &path).await;
        assert!(value.to_string().contains(&original));
    }
    assert!(
        sqlx::query("UPDATE content.posts SET poster_id='FORGEDID' WHERE id=$1")
            .bind(thread)
            .execute(&public)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE content.boards SET user_ids=false WHERE slug=$1")
            .bind(&board)
            .execute(&public)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT content.apply_poster_id()")
            .execute(&public)
            .await
            .is_err()
    );
    sqlx::query("UPDATE content.boards SET user_ids=false WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let plain = post(&no_key, &board, thread, Some("192.0.2.10:9000")).await["pid"]
        .as_i64()
        .unwrap();
    assert!(
        board_store::find_post(&public, &board, plain)
            .await
            .unwrap()
            .poster_id
            .is_none()
    );
    let (_, boards) = get(&api, "/boards.json").await;
    assert!(
        boards["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["board"] == board)
            .unwrap()
            .get("user_ids")
            .is_none()
    );
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
}
