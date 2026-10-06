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
async fn post(app: &Router, board: &str, parent: i64, peer: Option<&str>) -> Value {
    post_with_options(app, board, parent, peer, "").await
}
async fn post_with_options(
    app: &Router,
    board: &str,
    parent: i64,
    peer: Option<&str>,
    email: &str,
) -> Value {
    post_response(
        app,
        board,
        parent,
        peer,
        email,
        if peer.is_none() { 503 } else { 200 },
    )
    .await
}
async fn post_response(
    app: &Router,
    board: &str,
    parent: i64,
    peer: Option<&str>,
    email: &str,
    expected_status: u16,
) -> Value {
    let mut request = Request::post(format!("/{board}/post"))
        .header("origin", "https://boards.example.com")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .header("x-forwarded-for", "198.51.100.200")
        .header("x-board-client-ip", "198.51.100.201")
        .body(Body::from(format!("resto={parent}&name=Owned&sub=Owned&com=Owned+poster+identity&pwd=owned-poster-password&email={email}"))).unwrap();
    if let Some(peer) = peer {
        request.extensions_mut().insert(axum::extract::ConnectInfo(
            peer.parse::<SocketAddr>().unwrap(),
        ));
    }
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), expected_status);
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}

#[tokio::test]
async fn source_sage_ids_follow_locked_heaven_policy_and_preserve_saved_fields() {
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
    // Keep the saved OP identity-policy matrix on this owned board without
    // letting the unrelated actor quota preempt its field assertions.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Owned source sage IDs','Synthetic fixture',1000,100,100,100,10,2,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    let key = fixture_key();
    let (app, api) = board_public::routers_with_options(public.clone(), options(Some(key.clone())));
    let reference: Value = serde_json::from_str(include_str!(
        "../../../fixtures/poster-id-display-reference.json"
    ))
    .unwrap();
    assert_eq!(reference["cases"].as_array().unwrap().len(), 112);
    let thread = post(&app, &board, 0, Some("192.0.2.10:9000")).await["pid"]
        .as_i64()
        .unwrap();
    let mut saved: Vec<(i64, Option<String>)> = vec![(thread, None)];
    for (index, row) in reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["capcode"] == "none")
        .enumerate()
    {
        sqlx::query("UPDATE content.boards SET user_ids=$2,meta_board=$3,poster_id_no_heaven=$4 WHERE slug=$1")
            .bind(&board).bind(row["enabled"].as_bool().unwrap()).bind(row["meta_board"].as_bool().unwrap())
            .bind(row["no_heaven"].as_bool().unwrap()).execute(&owner).await.unwrap();
        let peer = if index % 2 == 0 {
            "192.0.2.10:9000"
        } else {
            "192.0.2.11:9000"
        };
        let email = if row["sage"] == true { "sage" } else { "" };
        let result = post_with_options(&app, &board, thread, Some(peer), email).await;
        assert!(result.get("error").is_none(), "{row}: {result}");
        let id = result["pid"].as_i64().unwrap();
        let expected = row["expected"].as_str().map(|label| {
            if label == reference["network_label_stub"].as_str().unwrap() {
                key.label(&board, thread, peer.parse::<SocketAddr>().unwrap().ip())
                    .unwrap()
            } else {
                label.to_owned()
            }
        });
        let actual = board_store::find_post(&public, &board, id).await.unwrap();
        assert_eq!(actual.poster_id, expected, "{row}");
        assert!(actual.capcode.is_none());
        saved.push((id, expected));
    }
    assert_eq!(saved.len(), 17);
    let contexts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.poster_contexts WHERE thread_id=$1")
            .bind(thread)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(contexts, 17);

    sqlx::query("UPDATE content.boards SET user_ids=true,meta_board=false,poster_id_no_heaven=false WHERE slug=$1")
        .bind(&board).execute(&owner).await.unwrap();
    let second_heaven = post_with_options(&app, &board, thread, Some("192.0.2.11:9000"), "sage")
        .await["pid"]
        .as_i64()
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, second_heaven)
            .await
            .unwrap()
            .poster_id
            .as_deref(),
        Some("Heaven")
    );
    saved.push((second_heaven, Some("Heaven".to_owned())));
    let no_key = board_public::routers_with_options(public.clone(), options(None)).0;
    assert_eq!(
        post_response(
            &no_key,
            &board,
            thread,
            Some("192.0.2.11:9000"),
            "sage",
            503
        )
        .await["error"],
        "Posting identity is unavailable."
    );
    assert_eq!(
        post_with_options(&app, &board, thread, None, "sage").await["error"],
        "Posting identity is unavailable."
    );
    let mut policy = owner.begin().await.unwrap();
    sqlx::query("SELECT poster_id_no_heaven FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .fetch_one(&mut *policy)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *policy)
        .await
        .unwrap();
    let waiting_app = app.clone();
    let waiting_board = board.clone();
    let waiter = tokio::spawn(async move {
        post_with_options(
            &waiting_app,
            &waiting_board,
            thread,
            Some("192.0.2.10:9000"),
            "sage",
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND usename='board_public' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(&owner).await.unwrap();
            if blocked {break;}
            assert!(!waiter.is_finished(),"Posting bypassed the owned ID policy lock");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("actual poster-ID policy lock wait");
    sqlx::query("UPDATE content.boards SET poster_id_no_heaven=true WHERE slug=$1")
        .bind(&board)
        .execute(&mut *policy)
        .await
        .unwrap();
    policy.commit().await.unwrap();
    let waited = waiter.await.unwrap();
    assert!(waited.get("error").is_none());
    let id = waited["pid"].as_i64().unwrap();
    let network = key
        .label(&board, thread, "192.0.2.10".parse().unwrap())
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, id)
            .await
            .unwrap()
            .poster_id
            .as_deref(),
        Some(network.as_str())
    );
    saved.push((id, Some(network)));

    for enabled in [false, true] {
        sqlx::query("UPDATE content.boards SET user_ids=$2,meta_board=true,poster_id_no_heaven=true WHERE slug=$1")
            .bind(&board).bind(enabled).execute(&owner).await.unwrap();
        for router in [&app, &api] {
            for path in [
                format!("/{board}/thread/{thread}.json"),
                format!("/{board}/thread/{thread}-tail.json"),
            ] {
                let (_, value) = get(router, &path).await;
                for post in value["posts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|post| post.get("resto").is_some())
                {
                    let expected = saved.iter().find(|(id, _)| post["no"] == *id).unwrap();
                    assert_eq!(post["id"].as_str(), expected.1.as_deref());
                }
                if !path.contains("-tail") {
                    assert_eq!(value["posts"][0]["unique_ips"], 2);
                }
            }
            for path in [format!("/{board}/1.json"), format!("/{board}/catalog.json")] {
                let (_, value) = get(router, &path).await;
                if path.ends_with("/1.json") {
                    for post in value["threads"][0]["posts"].as_array().unwrap() {
                        let expected = saved.iter().find(|(id, _)| post["no"] == *id).unwrap();
                        assert_eq!(post["id"].as_str(), expected.1.as_deref());
                    }
                } else {
                    for post in value[0]["threads"][0]["last_replies"].as_array().unwrap() {
                        let expected = saved.iter().find(|(id, _)| post["no"] == *id).unwrap();
                        assert_eq!(post["id"].as_str(), expected.1.as_deref());
                    }
                }
            }
        }
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{board}/thread/{thread}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            to_bytes(response.into_body(), 4_194_304)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("<span class=\"hand\">Heaven</span>"));
        assert!(!html.contains("192.0.2.10"));
    }
    let mut forged = public.begin().await.unwrap();
    sqlx::query("SELECT set_config('board.poster_id','Heaven',true),set_config('board.post_sage','true',true)")
        .execute(&mut *forged).await.unwrap();
    let error = sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Owned','','Owned invalid network label')")
        .bind(&board).bind(thread).execute(&mut *forged).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    forged.rollback().await.unwrap();
    for row in reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["capcode"] == "none")
    {
        let enabled = row["enabled"].as_bool().unwrap();
        sqlx::query("UPDATE content.boards SET user_ids=$2,meta_board=$3,poster_id_no_heaven=$4 WHERE slug=$1")
            .bind(&board).bind(enabled).bind(row["meta_board"].as_bool().unwrap())
            .bind(row["no_heaven"].as_bool().unwrap()).execute(&owner).await.unwrap();
        let email = if row["sage"] == true { "sage" } else { "" };
        let op = post_with_options(&app, &board, 0, Some("192.0.2.10:9000"), email).await["pid"]
            .as_i64()
            .unwrap();
        let network = key
            .label(&board, op, "192.0.2.10".parse().unwrap())
            .unwrap();
        let expected_saved = row["expected"].as_str().map(|label| {
            if label == reference["network_label_stub"].as_str().unwrap() {
                network.clone()
            } else {
                label.to_owned()
            }
        });
        let persisted = board_store::find_post(&public, &board, op).await.unwrap();
        assert_eq!(persisted.poster_id, expected_saved);
        assert_eq!(
            persisted.json_op_poster_id,
            enabled.then(|| network.clone())
        );
        for router in [&app, &api] {
            for path in [
                format!("/{board}/thread/{op}.json"),
                format!("/{board}/1.json"),
                format!("/{board}/catalog.json"),
            ] {
                let (_, value) = get(router, &path).await;
                let projected = if path.contains("/thread/") {
                    &value["posts"][0]
                } else if path.ends_with("/1.json") {
                    &value["threads"][0]["posts"][0]
                } else {
                    &value[0]["threads"][0]
                };
                assert_eq!(projected["no"], op);
                assert_eq!(
                    projected["id"].as_str(),
                    enabled.then_some(network.as_str())
                );
                assert!(projected.get("json_op_poster_id").is_none());
            }
        }
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{board}/thread/{op}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = String::from_utf8(
            to_bytes(response.into_body(), 4_194_304)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        if let Some(label) = expected_saved {
            assert!(html.contains(&format!("<span class=\"hand\">{label}</span>")));
        } else {
            assert!(!html.contains("posteruid"));
        }
        if enabled && row["sage"] == true && row["meta_board"] == false && row["no_heaven"] == false
        {
            for router in [&app, &api] {
                sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600,archive_limit=100 WHERE slug=$1")
                    .bind(&board).execute(&owner).await.unwrap();
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                    .bind(op).execute(&owner).await.unwrap();
                let (_, value) = get(router, &format!("/{board}/thread/{op}.json")).await;
                assert!(value["posts"][0].get("id").is_none());
            }
            let preserved = board_store::find_post(&public, &board, op).await.unwrap();
            assert_eq!(preserved.poster_id.as_deref(), Some("Heaven"));
            assert_eq!(
                preserved.json_op_poster_id.as_deref(),
                Some(network.as_str())
            );
        }
    }
    sqlx::query(
        "UPDATE content.boards SET archive_retention_seconds=3600,archive_limit=10 WHERE slug=$1",
    )
    .bind(&board)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(thread).execute(&owner).await.unwrap();
    for router in [&app, &api] {
        let (_, value) = get(router, &format!("/{board}/thread/{thread}.json")).await;
        assert!(
            value["posts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|post| post.get("id").is_none())
        );
    }
    let persisted: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id,poster_id FROM content.posts WHERE board=$1 AND thread_id=$2 ORDER BY id",
    )
    .bind(&board)
    .bind(thread)
    .fetch_all(&owner)
    .await
    .unwrap();
    assert_eq!(persisted, saved);
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
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,user_ids,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned poster IDs','Synthetic fixture',1000,100,100,100,10,true,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let key = fixture_key();
    let (app, api) = board_public::routers_with_options(public.clone(), options(Some(key.clone())));
    let no_key = board_public::routers_with_options(public.clone(), options(None)).0;
    assert_eq!(
        post_response(&no_key, &board, 0, Some("192.0.2.10:9000"), "", 503).await["error"],
        "Posting identity is unavailable."
    );
    assert_eq!(
        post(&app, &board, 0, None).await["error"],
        "Posting identity is unavailable."
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
        key.label(&board, thread, "192.0.2.10".parse().unwrap())
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
    assert_eq!(
        post_response(&no_key, &board, thread, Some("192.0.2.10:9000"), "", 503).await["error"],
        "Posting identity is unavailable."
    );
    let plain = post(&app, &board, thread, Some("192.0.2.10:9000")).await["pid"]
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
