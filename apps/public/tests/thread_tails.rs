#![cfg(feature = "database-tests")]
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn read(
    app: &axum::Router,
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
fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}
async fn add(pool: &sqlx::PgPool, slug: &str, parent: i64) -> i64 {
    board_store::create_post(
        pool,
        slug,
        parent,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: "Owned tail test".into(),
            comment: "Synthetic tail reply".into(),
            deletion_hash: "owned-fixture-not-a-password-hash".into(),
            sage: false,
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn persisted_tail_threshold_counts_cache_policy_and_privilege_boundaries() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = sqlx::PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug = format!(
        "ut{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES ($1,'Owned tail contract','Synthetic tail HTTP fixture',4000,1000,300,100,10,2)").bind(&slug).execute(&owner).await.unwrap();
    let id = add(&public, &slug, 0).await;
    let mut ids = vec![id];
    for _ in 0..3 {
        ids.push(add(&public, &slug, id).await);
    }
    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let full = format!("/{slug}/thread/{id}.json");
    let tail = format!("/{slug}/thread/{id}-tail.json");
    let projection = format!("/_watch/{slug}/thread/{id}/posts-tail");
    assert_eq!(
        read(&web, "GET", &tail, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        read(&web, "GET", &projection, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert!(
        json(&read(&web, "GET", &full, None).await.2)["posts"][0]
            .get("tail_size")
            .is_none()
    );
    ids.push(add(&public, &slug, id).await);
    let (_, full_headers, full_bytes) = read(&web, "GET", &full, None).await;
    assert_eq!(json(&full_bytes)["posts"][0]["tail_size"], 2);
    for app in [&web, &api] {
        let (status, headers, bytes) = read(app, "GET", &tail, None).await;
        assert_eq!(status, StatusCode::OK);
        let value = json(&bytes);
        let posts = value["posts"].as_array().unwrap();
        assert_eq!(posts.len(), 3);
        assert_eq!(posts[0]["tail_id"], ids[2]);
        assert_eq!(posts[0]["replies"], 4);
        assert_eq!(posts[0]["images"], 0);
        assert_eq!(posts[0]["no"], id);
        assert!(posts[0].get("com").is_none());
        assert!(posts[0].get("name").is_none());
        assert!(posts[0].get("resto").is_none());
        assert_eq!(posts[1]["no"], ids[3]);
        assert_eq!(posts[2]["no"], ids[4]);
        let tag = headers["etag"].to_str().unwrap();
        assert_ne!(headers["etag"], full_headers["etag"]);
        let (status, head, bytes) = read(app, "HEAD", &tail, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(head["etag"], headers["etag"]);
        assert!(bytes.is_empty());
        let (status, _, bytes) = read(app, "GET", &tail, Some(tag)).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(bytes.is_empty());
        assert_eq!(
            read(
                app,
                "GET",
                &tail,
                Some(full_headers["etag"].to_str().unwrap())
            )
            .await
            .0,
            StatusCode::OK
        );
        for method in ["POST", "PUT", "DELETE"] {
            assert_eq!(
                read(app, method, &tail, None).await.0,
                StatusCode::METHOD_NOT_ALLOWED
            );
        }
    }
    let (status, headers, bytes) = read(&web, "GET", &projection, None).await;
    assert_eq!(status, StatusCode::OK);
    let value = json(&bytes);
    assert_eq!(value["version"], 2);
    assert_eq!(value["tail_id"], ids[2].to_string());
    assert_eq!(value["posts"][1]["no"], ids[3].to_string());
    assert_eq!(value["replies"], 4);
    assert_eq!(
        read(
            &web,
            "GET",
            &projection,
            Some(headers["etag"].to_str().unwrap())
        )
        .await
        .0,
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        read(&api, "GET", &projection, None).await.0,
        StatusCode::NOT_FOUND
    );
    let html = read(&web, "GET", &format!("/{slug}/thread/{id}"), None)
        .await
        .2;
    assert!(
        String::from_utf8(html)
            .unwrap()
            .contains("data-tail-size=\"2\"")
    );

    // Both application identities exercise real denied writes; the operator can
    // change policy, and even an unchanged thread timestamp cannot hide that change.
    for pool in [&public, &staff] {
        let error = sqlx::query("UPDATE content.boards SET json_tail_size=1 WHERE slug=$1")
            .bind(&slug)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        let error = sqlx::query("UPDATE content.threads SET undead=true WHERE board=$1 AND id=$2")
            .bind(&slug)
            .bind(id)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    for invalid in [-1, 501] {
        let error = sqlx::query("UPDATE content.boards SET json_tail_size=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(invalid)
            .execute(&owner)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    sqlx::query("UPDATE content.boards SET json_tail_size=3 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (status, changed, bytes) = read(
        &web,
        "GET",
        &full,
        Some(full_headers["etag"].to_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(changed["etag"], full_headers["etag"]);
    assert!(json(&bytes)["posts"][0].get("tail_size").is_none());
    assert_eq!(
        read(&web, "GET", &tail, None).await.0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE content.boards SET json_tail_size=2 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    board_store::delete_post(&public, &slug, ids[1])
        .await
        .unwrap();
    assert_eq!(
        read(&web, "GET", &tail, None).await.0,
        StatusCode::NOT_FOUND
    );
    for _ in 0..5 {
        ids.push(add(&public, &slug, id).await);
    }
    for (sticky, undead, expected) in [(true, false, 2), (false, true, 2), (true, true, 4)] {
        sqlx::query("UPDATE content.threads SET sticky=$3,undead=$4 WHERE board=$1 AND id=$2")
            .bind(&slug)
            .bind(id)
            .bind(sticky)
            .bind(undead)
            .execute(&owner)
            .await
            .unwrap();
        let snapshot = board_store::thread_snapshot_selection(&public, &slug, id, true)
            .await
            .unwrap();
        assert_eq!(snapshot.replies, 8);
        assert_eq!(snapshot.tail_size, expected);
        assert_eq!(snapshot.posts.len(), expected + 1);
        assert_eq!(snapshot.tail_id, Some(ids[ids.len() - expected - 1]));
    }
    let writer_pool = public.clone();
    let writer_slug = slug.clone();
    let writer = tokio::spawn(async move {
        for _ in 0..12 {
            add(&writer_pool, &writer_slug, id).await;
        }
    });
    for _ in 0..32 {
        let selected = board_store::thread_snapshot_selection(&public, &slug, id, true)
            .await
            .unwrap();
        let complete = board_store::thread_snapshot(&public, &slug, id)
            .await
            .unwrap();
        let last = selected.posts.last().unwrap().id;
        let end = complete
            .posts
            .iter()
            .position(|post| post.id == last)
            .unwrap();
        assert_eq!(selected.replies, end);
        assert_eq!(
            selected.tail_id,
            Some(complete.posts[end - selected.tail_size].id)
        );
        assert_eq!(selected.thread.reply_count as usize, selected.replies + 1);
    }
    writer.await.unwrap();
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    staff.close().await;
    owner.close().await;
}
