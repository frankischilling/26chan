#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use board_store::NewPost;
use chrono::{DateTime, Timelike, Utc};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

async fn get(
    app: &axum::Router,
    path: &str,
    header: Option<(&str, &str)>,
) -> axum::response::Response {
    let mut request = Request::get(path);
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn exercise(public: PgPool, slug: String) {
    let post = NewPost {
        name: "Anonymous".into(),
        subject: "Owned clocks".into(),
        comment: "Owned timestamp fixture".into(),
        deletion_hash: "unused-owned-hash".into(),
        sage: false,
    };
    let requested = DateTime::from_timestamp(1_700_000_000, 987_654_321).unwrap();
    let id = board_store::create_post_with_attachment_at(&public, &slug, 0, &post, None, requested)
        .await
        .unwrap();
    let before = board_store::thread_snapshot(&public, &slug, id)
        .await
        .unwrap();
    let seconds = requested.with_nanosecond(0).unwrap();
    assert_eq!(before.posts[0].created_at, seconds);
    assert_eq!(before.thread.created_at, seconds);
    assert_eq!(before.thread.modified_at, seconds);
    assert!(
        before.thread.bumped_at > requested,
        "root ordering uses the database clock"
    );
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let tail_path = format!("/{slug}/thread/{id}-tail.json");
    assert_eq!(get(&app, &tail_path, None).await.status(), 404);
    // A one-reply tail is available only after two surviving replies.
    // Establish that source threshold before testing its cache validators.
    for _ in 0..2 {
        board_store::create_post_with_attachment_at(&public, &slug, id, &post, None, requested)
            .await
            .unwrap();
    }
    let paths = [
        format!("/{slug}/thread/{id}.json"),
        format!("/{slug}/thread/{id}-tail.json"),
        format!("/_watch/{slug}/thread/{id}/posts"),
    ];
    let mut validators = Vec::new();
    for path in &paths {
        let response = get(&app, path, None).await;
        assert_eq!(response.status(), 200, "{path}");
        validators.push((
            response.headers()["etag"].to_str().unwrap().to_owned(),
            response.headers()["last-modified"]
                .to_str()
                .unwrap()
                .to_owned(),
        ));
    }
    // The same source second still changes the representation and its ETag.
    let same =
        board_store::create_post_with_attachment_at(&public, &slug, id, &post, None, requested)
            .await
            .unwrap();
    for (path, (etag, modified)) in paths.iter().zip(&validators) {
        let changed = get(&app, path, Some(("if-none-match", etag))).await;
        assert_eq!(changed.status(), 200);
        assert_ne!(changed.headers()["etag"], etag);
        let current = changed.headers()["etag"].to_str().unwrap();
        assert_eq!(
            get(&app, path, Some(("if-none-match", current)))
                .await
                .status(),
            304
        );
        assert_eq!(
            get(&app, path, Some(("if-modified-since", modified)))
                .await
                .status(),
            200
        );
    }
    // An older request can obtain the mutation lock last. Source last_modified
    // follows that request, while HTTP validators follow the database change.
    let later = board_store::thread(&public, &slug, id).await.unwrap();
    let mut sage = post.clone();
    sage.sage = true;
    let earlier = requested - chrono::Duration::seconds(100);
    let late =
        board_store::create_post_with_attachment_at(&public, &slug, id, &sage, None, earlier)
            .await
            .unwrap();
    let after = board_store::thread_snapshot(&public, &slug, id)
        .await
        .unwrap();
    assert_eq!(
        after
            .posts
            .iter()
            .find(|p| p.id == same)
            .unwrap()
            .created_at,
        seconds
    );
    assert_eq!(
        after
            .posts
            .iter()
            .find(|p| p.id == late)
            .unwrap()
            .created_at,
        earlier.with_nanosecond(0).unwrap()
    );
    assert_eq!(after.thread.modified_at.timestamp(), earlier.timestamp());
    assert!(after.thread.modified_at < later.modified_at);
    assert!(after.thread.http_modified_at > later.http_modified_at);
    assert_eq!(
        after.thread.bumped_at, later.bumped_at,
        "sage preserves the root clock"
    );
    let response = get(&app, &paths[0], None).await;
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    for (number, time) in [
        (id, seconds),
        (same, seconds),
        (late, earlier.with_nanosecond(0).unwrap()),
    ] {
        let entry = value["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["no"] == number)
            .unwrap();
        assert_eq!(entry["time"], time.timestamp());
    }
    let response = get(&app, &format!("/{slug}/thread/{id}"), None).await;
    assert_eq!(response.status(), 200);
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains(&format!("datetime=\"{}\"", seconds.to_rfc3339())));
    assert!(html.contains(&format!(
        "datetime=\"{}\"",
        earlier.with_nanosecond(0).unwrap().to_rfc3339()
    )));
    for (path, (_, modified)) in paths.iter().zip(&validators) {
        assert_eq!(
            get(&app, path, Some(("if-modified-since", modified)))
                .await
                .status(),
            200
        );
    }
    for suffix in ["threads.json", "catalog.json"] {
        let response = get(&app, &format!("/{slug}/{suffix}"), None).await;
        assert_eq!(response.status(), 200);
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(value[0]["threads"][0]["last_modified"], earlier.timestamp());
    }
    // All public posting aliases/encodings capture the clock before consuming
    // an actual delayed body. Neither body fields nor headers choose the time.
    for route in ["post", "imgboard.php"] {
        for multipart in [false, true] {
            let polled = Arc::new(AtomicI64::new(0));
            let seen = polled.clone();
            let body = Body::from_stream(futures_util::stream::once(async move {
                let start = Utc::now().timestamp();
                seen.store(start, Ordering::SeqCst);
                while Utc::now().timestamp() <= start {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                let value = if multipart {
                    format!(
                        "--clock\r\nContent-Disposition: form-data; name=\"resto\"\r\n\r\n{id}\r\n--clock\r\nContent-Disposition: form-data; name=\"com\"\r\n\r\nOwned delayed clock\r\n--clock\r\nContent-Disposition: form-data; name=\"pwd\"\r\n\r\nowned-secret\r\n--clock\r\nContent-Disposition: form-data; name=\"time\"\r\n\r\n1\r\n--clock--\r\n"
                    )
                } else {
                    format!("resto={id}&com=Owned+delayed+clock&pwd=owned-secret&time=1")
                };
                Ok::<_, std::io::Error>(bytes::Bytes::from(value))
            }));
            let started = Utc::now().timestamp();
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/{slug}/{route}"))
                        .header("origin", "http://127.0.0.1:3000")
                        .header("accept", "application/json")
                        .header("x-request-start", "1")
                        .header(
                            "content-type",
                            if multipart {
                                "multipart/form-data; boundary=clock"
                            } else {
                                "application/x-www-form-urlencoded"
                            },
                        )
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let value: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            let rid = value["pid"].as_i64().unwrap();
            let snapshot = board_store::thread_snapshot(&public, &slug, id)
                .await
                .unwrap();
            let saved = snapshot
                .posts
                .iter()
                .find(|p| p.id == rid)
                .unwrap()
                .created_at;
            assert!((started..=polled.load(Ordering::SeqCst)).contains(&saved.timestamp()));
            assert_eq!(saved.nanosecond(), 0);
            assert!(saved.timestamp() < Utc::now().timestamp());
            assert_eq!(snapshot.thread.modified_at, saved);
        }
    }
}

#[tokio::test]
async fn source_posting_seconds_and_http_change_clocks_are_independent() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES($1,'Posting times','Owned synthetic fixture',1000,100,100,10,10,1)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for query in [
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
    owner.close().await;
    result.unwrap();
}
