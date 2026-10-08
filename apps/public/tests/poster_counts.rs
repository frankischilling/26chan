#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn fixture_key() -> std::sync::Arc<board_domain::poster_id::PosterIdKey> {
    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::sync::Arc::new(board_domain::poster_id::PosterIdKey::parse(&encoded).unwrap())
}

fn options(
    key: Option<Arc<board_domain::poster_id::PosterIdKey>>,
) -> board_public::PublicRouterOptions {
    board_public::PublicRouterOptions {
        country_database: None,
        origin: "https://boards.example.com".into(),
        production: true,
        media: None,
        limits: board_config::PublicRequestLimits::default(),
        proxy_uid: None,
        tripcode_key: None,
        poster_id_key: key,
    }
}
async fn post(app: &Router, board: &str, parent: i64, peer: &str) -> i64 {
    let response = app
        .clone()
        .oneshot(post_request(board, parent, Some(peer)))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    serde_json::from_slice::<Value>(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()["pid"].as_i64().unwrap()
}
fn post_request(board: &str, parent: i64, peer: Option<&str>) -> Request<Body> {
    let mut request = Request::post(format!("/{board}/post"))
        .header("origin", "https://boards.example.com")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .header("x-forwarded-for", "198.51.100.200")
        .header("x-board-client-ip", "198.51.100.201")
        .body(Body::from(format!(
            "resto={parent}&name=Owned&sub=Owned&com=Owned+count&pwd=owned-counter-password"
        )))
        .unwrap();
    if let Some(peer) = peer {
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<SocketAddr>().unwrap(),
        ));
    }
    request
}
async fn get(app: &Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    serde_json::from_slice(&to_bytes(response.into_body(), 4_194_304).await.unwrap()).unwrap()
}
async fn count(public: &sqlx::PgPool, board: &str, thread: i64) -> Option<i32> {
    sqlx::query_scalar("SELECT content.unique_posters($1,$2)")
        .bind(board)
        .bind(thread)
        .fetch_one(public)
        .await
        .unwrap()
}

#[tokio::test]
async fn complete_private_contexts_supply_counts_and_incomplete_or_archived_history_omits_them() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // One connection proves transaction-local fingerprint values cannot leak.
    let public = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,json_tail_size,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned counters','Synthetic fixture',1000,100,100,100,10,3600,1,0,0,0)")
        .bind(&board).execute(&owner).await.unwrap();
    let outcome = tokio::spawn(exercise(owner.clone(), public.clone(), board.clone())).await;
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
    outcome.unwrap();
}

async fn exercise(owner: sqlx::PgPool, public: sqlx::PgPool, board: String) {
    let (app, api) =
        board_public::routers_with_options(public.clone(), options(Some(fixture_key())));
    let no_key = board_public::routers_with_options(public.clone(), options(None)).0;
    let rotated =
        board_public::routers_with_options(public.clone(), options(Some(fixture_key()))).0;
    let thread = post(&app, &board, 0, "192.0.2.10:9000").await;
    assert_eq!(count(&public, &board, thread).await, Some(1));
    let (_, fingerprint_bytes): (i64, i32) = sqlx::query_as("SELECT post_id,octet_length(fingerprint) FROM post_secrets.poster_contexts WHERE post_id=$1")
        .bind(thread).fetch_one(&owner).await.unwrap();
    assert_eq!(fingerprint_bytes, 32);
    let same = post(&app, &board, thread, "[::ffff:192.0.2.10]:9001").await;
    assert_eq!(count(&public, &board, thread).await, Some(1));
    let other = post(&app, &board, thread, "192.0.2.11:9000").await;
    assert_eq!(count(&public, &board, thread).await, Some(2));
    for listener in [&app, &api] {
        let value = get(listener, &format!("/{board}/thread/{thread}.json")).await;
        assert_eq!(value["posts"][0]["unique_ips"], 2);
        assert!(value["posts"][0].get("id").is_none());
        assert!(value["posts"][1].get("unique_ips").is_none());
        assert!(!value.to_string().contains("192.0.2.10"));
        let tail = get(listener, &format!("/{board}/thread/{thread}-tail.json")).await;
        assert_eq!(tail["posts"].as_array().unwrap().len(), 2);
        assert_eq!(tail["posts"][0]["unique_ips"], 2);
        assert_eq!(tail["posts"][1]["no"], other);
        assert!(tail["posts"][1].get("unique_ips").is_none());
        let page = get(listener, &format!("/{board}/1.json")).await;
        assert_eq!(page["threads"][0]["posts"][0]["unique_ips"], 2);
        let catalog = get(listener, &format!("/{board}/catalog.json")).await;
        assert_eq!(catalog[0]["threads"][0]["unique_ips"], 2);
    }
    assert_eq!(
        get(&app, &format!("/_watch/{board}/thread/{thread}/stats")).await["unique_ips"],
        2
    );
    board_store::delete_post(&public, &board, other)
        .await
        .unwrap();
    assert_eq!(count(&public, &board, thread).await, Some(1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM post_secrets.poster_contexts WHERE post_id=$1"
        )
        .bind(other)
        .fetch_one(&owner)
        .await
        .unwrap(),
        0
    );
    let missing_key = no_key
        .clone()
        .oneshot(post_request(&board, thread, Some("192.0.2.12:9000")))
        .await
        .unwrap();
    assert_eq!(missing_key.status(), 503);
    let unavailable: Value =
        serde_json::from_slice(&to_bytes(missing_key.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(unavailable["error"], "Posting identity is unavailable.");
    assert_eq!(count(&public, &board, thread).await, Some(1));
    // Model a legacy post without a captured context through the owner fixture,
    // while every live submission supplies the mandatory posting identity.
    let gap = post(&app, &board, thread, "192.0.2.12:9000").await;
    sqlx::query("DELETE FROM post_secrets.poster_contexts WHERE post_id=$1 AND thread_id=$2")
        .bind(gap)
        .bind(thread)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(count(&public, &board, thread).await, None);
    assert!(
        get(&api, &format!("/{board}/thread/{thread}.json")).await["posts"][0]
            .get("unique_ips")
            .is_none()
    );
    board_store::delete_post(&public, &board, gap)
        .await
        .unwrap();
    assert_eq!(count(&public, &board, thread).await, Some(1));
    let changed = post(&rotated, &board, thread, "192.0.2.10:9000").await;
    assert_eq!(count(&public, &board, thread).await, None);
    board_store::delete_post(&public, &board, changed)
        .await
        .unwrap();
    assert_eq!(count(&public, &board, thread).await, Some(1));
    let unverified = app
        .clone()
        .oneshot(post_request(&board, thread, None))
        .await
        .unwrap();
    assert_eq!(unverified.status(), 503);
    assert_eq!(count(&public, &board, thread).await, Some(1));
    let historical = post(&app, &board, 0, "192.0.2.10:9000").await;
    sqlx::query("DELETE FROM post_secrets.poster_contexts WHERE post_id=$1 AND thread_id=$1")
        .bind(historical)
        .execute(&owner)
        .await
        .unwrap();
    post(&app, &board, historical, "192.0.2.10:9000").await;
    assert_eq!(count(&public, &board, historical).await, None);
    for (fingerprint, epoch) in [
        ("1".repeat(64), String::new()),
        (String::new(), "1".repeat(64)),
        ("A".repeat(64), "1".repeat(64)),
        ("1".repeat(63), "1".repeat(64)),
        ("1".repeat(64), "g".repeat(64)),
    ] {
        let mut tx = public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.poster_fingerprint',$1,true),set_config('board.poster_epoch',$2,true)")
            .bind(fingerprint).bind(epoch).execute(&mut *tx).await.unwrap();
        let error = sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Owned','','Malformed capture')")
            .bind(&board).bind(thread).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        tx.rollback().await.unwrap();
    }
    assert_eq!(count(&public, &board, thread).await, Some(1));
    for variable in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "MONITOR_DATABASE_URL",
    ] {
        let runtime = sqlx::PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let error = sqlx::query("SELECT fingerprint FROM post_secrets.poster_contexts")
            .execute(&runtime)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{variable}"
        );
        runtime.close().await;
    }
    for query in [
        "INSERT INTO post_secrets.poster_contexts(post_id,thread_id,fingerprint,epoch) VALUES(1,1,'','')",
        "DELETE FROM post_secrets.poster_contexts",
        "SET ROLE board_poster_count_owner",
        "SELECT content.record_poster_context()",
    ] {
        let error = sqlx::query(query).execute(&public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    sqlx::query("UPDATE content.threads SET closed=true,archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(thread).execute(&owner).await.unwrap();
    assert_eq!(count(&public, &board, thread).await, None);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM post_secrets.poster_contexts WHERE thread_id=$1"
        )
        .bind(thread)
        .fetch_one(&owner)
        .await
        .unwrap(),
        0
    );
    assert!(
        get(&api, &format!("/{board}/thread/{thread}.json")).await["posts"][0]
            .get("unique_ips")
            .is_none()
    );
    assert!(
        !get(&app, &format!("/_watch/{board}/thread/{thread}/stats"))
            .await
            .to_string()
            .contains("unique_ips")
    );
    assert!(same > thread);
}
