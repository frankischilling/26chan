#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tower::ServiceExt;

async fn get(app: &axum::Router, path: &str) -> Vec<u8> {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

async fn coherent_during_commit(owner: PgPool, public: PgPool, slug: String, id: i64) {
    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let reader: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&public)
        .await
        .unwrap();
    let mut inconsistent = Vec::new();
    for delete in [false, true] {
        for (app, suffix) in [
            (&web, "catalog.json"),
            (&web, "threads.json"),
            (&web, "1.json"),
            (&api, "catalog.json"),
            (&api, "threads.json"),
            (&api, "1.json"),
            (&web, ""),
            (&web, "catalog"),
            (&web, "catalog?order=absdate"),
            (&web, "catalog?order=date"),
            (&web, "catalog?order=r&q=commit"),
        ] {
            sqlx::query(
                "UPDATE content.boards SET title='Before commit',bump_limit=50 WHERE slug=$1",
            )
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
            sqlx::query("UPDATE content.threads SET deleted=false,sticky=false,closed=false,reply_count=0,modified_at='2026-01-01T00:00:00Z' WHERE id=$1")
            .bind(id).execute(&owner).await.unwrap();
            sqlx::query("DELETE FROM content.posts WHERE board=$1 AND id<>$2")
                .bind(&slug)
                .bind(id)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE content.posts SET deleted=false,comment='Before commit' WHERE id=$1",
            )
            .bind(id)
            .execute(&owner)
            .await
            .unwrap();
            let path = format!("/{slug}/{suffix}");
            let before = get(app, &path).await;

            let mut writer = owner.begin().await.unwrap();
            sqlx::query("LOCK TABLE content.posts IN ACCESS EXCLUSIVE MODE")
                .execute(&mut *writer)
                .await
                .unwrap();
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            let commit = async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let blocked: bool =
                            sqlx::query_scalar("SELECT $1 = ANY(pg_blocking_pids($2))")
                                .bind(writer_pid)
                                .bind(reader)
                                .fetch_one(&owner)
                                .await
                                .unwrap();
                        if blocked {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .expect("the owned reader must reach the locked posts table");
                sqlx::query(
                    "UPDATE content.boards SET title='After commit',bump_limit=0 WHERE slug=$1",
                )
                .bind(&slug)
                .execute(&mut *writer)
                .await
                .unwrap();
                sqlx::query("UPDATE content.threads SET sticky=true,closed=true,reply_count=1,modified_at='2026-01-02T00:00:00Z' WHERE id=$1")
                .bind(id).execute(&mut *writer).await.unwrap();
                sqlx::query("UPDATE content.posts SET comment='After commit' WHERE id=$1")
                    .bind(id)
                    .execute(&mut *writer)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','A committed reply')")
                .bind(&slug).bind(id).execute(&mut *writer).await.unwrap();
                if delete {
                    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
                        .bind(id)
                        .execute(&mut *writer)
                        .await
                        .unwrap();
                    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1")
                        .bind(&slug)
                        .execute(&mut *writer)
                        .await
                        .unwrap();
                }
                writer.commit().await.unwrap();
            };
            let (during, ()) = tokio::join!(get(app, &path), commit);
            let after = get(app, &path).await;
            assert_ne!(before, after, "the committed control must change {suffix}");
            if during != before && during != after {
                inconsistent.push(format!("{suffix} delete={delete}"));
            }
        }
    }
    assert!(
        inconsistent.is_empty(),
        "responses combined different commits: {inconsistent:?}"
    );
    settled_contracts(&owner, &public, &slug, id).await;
}

async fn cached(app: &axum::Router, path: &str, etag: Option<&str>) -> (u16, String) {
    let mut request = Request::builder().uri(path);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        response.headers()["cache-control"],
        "public, max-age=0, must-revalidate"
    );
    (
        response.status().as_u16(),
        response.headers()["etag"].to_str().unwrap().to_owned(),
    )
}

async fn settled_contracts(owner: &PgPool, public: &PgPool, slug: &str, id: i64) {
    use board_store::{BoardSelection, StoreError, board_snapshot};
    use serde_json::{Value, json};
    sqlx::query("UPDATE content.boards SET threads_per_page=2,thread_limit=3 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET deleted=false,reply_count=9 WHERE id=$1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET deleted=false WHERE board=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    for _ in 0..8 {
        sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','A preview reply')")
            .bind(slug).bind(id).execute(owner).await.unwrap();
    }
    let deleted: i64 = sqlx::query_scalar("UPDATE content.posts SET deleted=true WHERE id=(SELECT min(id) FROM content.posts WHERE board=$1 AND id<>$2) RETURNING id")
        .bind(slug).bind(id).fetch_one(owner).await.unwrap();
    let mut other_ids = Vec::new();
    for _ in 0..2 {
        let other: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,bumped_at) VALUES($1,'2026-01-01T00:00:00Z') RETURNING id")
            .bind(slug).fetch_one(owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Another thread')")
            .bind(other).bind(slug).execute(owner).await.unwrap();
        other_ids.push(other);
    }
    let full = board_snapshot(public, slug, BoardSelection::All, Some(5))
        .await
        .unwrap();
    assert_eq!(
        full.threads.iter().map(|p| p.thread.id).collect::<Vec<_>>(),
        vec![id, other_ids[1], other_ids[0]]
    );
    assert!(!full.has_next);
    let preview = &full.threads[0];
    assert_eq!(
        preview.thread.reply_count, 9,
        "the lifetime count is not the visible count"
    );
    assert_eq!(preview.visible_posts, 9);
    assert_eq!(preview.posts.len(), 6);
    assert_eq!(preview.posts[0].id, id);
    assert!(preview.posts.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(!preview.posts.iter().any(|post| post.id == deleted));
    let last: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id DESC LIMIT 5")
        .bind(slug).bind(id).fetch_all(public).await.unwrap();
    assert_eq!(
        preview.posts[1..]
            .iter()
            .rev()
            .map(|post| post.id)
            .collect::<Vec<_>>(),
        last
    );
    assert_eq!(preview.latest_reply_id, last.first().copied());
    let first = board_snapshot(public, slug, BoardSelection::Page(1), None)
        .await
        .unwrap();
    assert!(first.has_next);
    assert_eq!(first.threads.len(), 2);
    assert!(first.threads.iter().all(|p| p.posts.is_empty()));
    assert_eq!(first.threads[0].visible_posts, 9);
    let second = board_snapshot(public, slug, BoardSelection::Page(2), Some(0))
        .await
        .unwrap();
    assert!(!second.has_next);
    assert_eq!(second.threads.len(), 1);
    assert_eq!(second.threads[0].posts.len(), 1);
    for page in [i64::MIN, 0, 3, i64::MAX] {
        assert!(matches!(
            board_snapshot(public, slug, BoardSelection::Page(page), Some(5)).await,
            Err(StoreError::PageNotFound)
        ));
    }
    for limit in [-1, 6, i64::MAX] {
        assert!(matches!(
            board_snapshot(public, slug, BoardSelection::All, Some(limit)).await,
            Err(StoreError::Invalid(_))
        ));
    }

    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    for app in [&web, &api] {
        let index: Value =
            serde_json::from_slice(&get(app, &format!("/{slug}/1.json")).await).unwrap();
        let posts = &index["threads"][0]["posts"];
        assert_eq!(posts.as_array().unwrap().len(), 6);
        assert_eq!(posts[0]["replies"], 8);
        assert_eq!(posts[0]["omitted_posts"], 3);
        let catalog: Value =
            serde_json::from_slice(&get(app, &format!("/{slug}/catalog.json")).await).unwrap();
        assert_eq!(catalog.as_array().unwrap().len(), 2);
        assert_eq!(catalog[0]["threads"].as_array().unwrap().len(), 2);
        assert_eq!(catalog[1]["threads"].as_array().unwrap().len(), 1);
        assert_eq!(
            catalog[0]["threads"][0]["last_replies"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        let second: Value =
            serde_json::from_slice(&get(app, &format!("/{slug}/2.json")).await).unwrap();
        assert_eq!(second["threads"].as_array().unwrap().len(), 1);
        for suffix in ["0.json", "3.json", "9223372036854775807.json"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/{slug}/{suffix}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 404);
            let body = String::from_utf8(
                response
                    .into_body()
                    .collect()
                    .await
                    .unwrap()
                    .to_bytes()
                    .to_vec(),
            )
            .unwrap();
            assert!(body.contains("Page not found."), "{suffix}: {body}");
        }
    }
    let html = String::from_utf8(get(&web, &format!("/{slug}/")).await).unwrap();
    let catalog_html = String::from_utf8(get(&web, &format!("/{slug}/catalog")).await).unwrap();
    assert!(catalog_html.contains(&format!("id=\"thread-{id}\"")));
    assert!(
        catalog_html.contains(&format!(
            "id=\"meta-{id}\" title=\"(R)eplies / (I)mage Replies\">R: <b>8</b>"
        )),
        "catalog must count visible replies, not the lifetime count of nine"
    );
    assert!(!catalog_html.contains("posts omitted"));
    assert!(!catalog_html.contains("class=\"postInfo\""));
    for suffix in ["2", "999"] {
        let response = web
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/{slug}/{suffix}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
        let body = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(body.contains("Page not found."));
    }
    assert_eq!(html.matches("replyContainer").count(), 3);
    assert!(html.contains("5 posts omitted"));
    assert!(!catalog_html.contains("replyContainer"));

    let mut validators = Vec::new();
    for app in [&web, &api] {
        for suffix in ["catalog.json", "threads.json", "1.json"] {
            let path = format!("/{slug}/{suffix}");
            let (status, etag) = cached(app, &path, None).await;
            assert_eq!(status, 200);
            assert_eq!(cached(app, &path, Some(&etag)).await.0, 304);
            validators.push((app, path, etag));
        }
    }
    sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    for (app, path, old_etag) in validators {
        let (status, etag) = cached(app, &path, Some(&old_etag)).await;
        assert_eq!(status, 200);
        assert_ne!(etag, old_etag);
        assert_eq!(cached(app, &path, Some(&etag)).await.0, 304);
        let empty: Value = serde_json::from_slice(&get(app, &path).await).unwrap();
        assert_eq!(
            empty,
            if path.ends_with("/1.json") {
                json!({"threads": []})
            } else {
                json!([])
            }
        );
    }
    let empty = board_snapshot(public, slug, BoardSelection::All, Some(5))
        .await
        .unwrap();
    assert!(empty.threads.is_empty());
    assert!(!empty.has_next);
    for suffix in ["", "catalog", "1"] {
        assert!(
            String::from_utf8(get(&web, &format!("/{slug}/{suffix}")).await)
                .unwrap()
                .contains("No threads yet")
        );
    }
}

#[tokio::test]
async fn board_representations_use_one_snapshot_during_a_committed_change() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // One actual public connection makes the lock witness identify this reader.
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(identity, "board_public");
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Before commit','Owned snapshot fixture',1000,100,50,10,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&slug)
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Snapshot','Before commit')")
        .bind(id).bind(&slug).execute(&owner).await.unwrap();

    // Capture assertion failures so the owned board and connections are cleaned.
    let result = tokio::spawn(coherent_during_commit(
        owner.clone(),
        public.clone(),
        slug.clone(),
        id,
    ))
    .await;
    public.close().await;
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
    result.unwrap();
}
