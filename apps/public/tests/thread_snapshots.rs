#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tower::ServiceExt;

async fn get(app: &axum::Router, path: &str) -> Vec<u8> {
    let response = request(app, path, None).await;
    assert_eq!(response.status(), 200, "{path}");
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

async fn request(app: &axum::Router, path: &str, etag: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn coherent_during_commit(owner: PgPool, control: PgPool, slug: String, id: i64) {
    let (web_control, api_control) =
        board_public::routers(control.clone(), "http://127.0.0.1:3000".into(), false);
    let mut inconsistent = Vec::new();
    for table_lock in [false, true] {
        for (api_only, json) in [(false, false), (false, true), (true, true)] {
            sqlx::query(
                "UPDATE content.boards SET title='Before commit',bump_limit=50 WHERE slug=$1",
            )
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
            sqlx::query("UPDATE content.threads SET reply_count=0,modified_at='2026-01-01T00:00:00Z' WHERE id=$1")
            .bind(id).execute(&owner).await.unwrap();
            sqlx::query("UPDATE content.posts SET comment='Before commit' WHERE id=$1")
                .bind(id)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query("DELETE FROM content.posts WHERE board=$1 AND id<>$2")
                .bind(&slug)
                .bind(id)
                .execute(&owner)
                .await
                .unwrap();
            let path = format!("/{slug}/thread/{id}{}", if json { ".json" } else { "" });
            let control_app = if api_only { &api_control } else { &web_control };
            let before = get(control_app, &path).await;

            // A fresh lazy pool has no setup release that could consume the barrier.
            // The first return is the old handler's board read, or the complete
            // snapshot transaction once board settings are part of that snapshot.
            let first = Arc::new(AtomicBool::new(!table_lock));
            let release_failed = Arc::new(AtomicBool::new(false));
            let reached = Arc::new(Notify::new());
            let resume = Arc::new(Notify::new());
            let public = PgPoolOptions::new()
                .min_connections(0)
                .max_connections(1)
                .after_connect(|connection, _| {
                    Box::pin(async move {
                        let identity: String = sqlx::query_scalar("SELECT current_user")
                            .fetch_one(connection)
                            .await?;
                        assert_eq!(identity, "board_public");
                        Ok(())
                    })
                })
                .after_release({
                    let first = first.clone();
                    let release_failed = release_failed.clone();
                    let reached = reached.clone();
                    let resume = resume.clone();
                    move |_, _| {
                        let pause = first.swap(false, Ordering::SeqCst);
                        let release_failed = release_failed.clone();
                        let reached = reached.clone();
                        let resume = resume.clone();
                        Box::pin(async move {
                            if pause {
                                reached.notify_one();
                                tokio::time::timeout(Duration::from_secs(5), resume.notified())
                                    .await
                                    .map_err(|_| {
                                        release_failed.store(true, Ordering::SeqCst);
                                        sqlx::Error::PoolTimedOut
                                    })?;
                            }
                            Ok(true)
                        })
                    }
                })
                .connect_lazy(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .unwrap();
            let (web, api) =
                board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
            let mut writer = owner.begin().await.unwrap();
            let mut reader_pid = 0_i32;
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            if table_lock {
                // This second barrier commits between queries within the transaction,
                // so a shared connection without repeatable-read isolation is insufficient.
                reader_pid = sqlx::query_scalar("SELECT pg_backend_pid()")
                    .fetch_one(&public)
                    .await
                    .unwrap();
                sqlx::query("LOCK TABLE content.posts IN ACCESS EXCLUSIVE MODE")
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
            let commit = async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    if table_lock {
                        loop {
                            let blocked: bool =
                                sqlx::query_scalar("SELECT $1 = ANY(pg_blocking_pids($2))")
                                    .bind(writer_pid)
                                    .bind(reader_pid)
                                    .fetch_one(&owner)
                                    .await
                                    .unwrap();
                            if blocked {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    } else {
                        reached.notified().await;
                    }
                })
                .await
                .expect("the owned reader must reach the selected commit barrier");
                sqlx::query(
                    "UPDATE content.boards SET title='After commit',bump_limit=0 WHERE slug=$1",
                )
                .bind(&slug)
                .execute(&mut *writer)
                .await
                .unwrap();
                sqlx::query("UPDATE content.threads SET reply_count=1,modified_at='2026-01-02T00:00:00Z' WHERE id=$1")
                .bind(id).execute(&mut *writer).await.unwrap();
                sqlx::query("UPDATE content.posts SET comment='After commit' WHERE id=$1")
                    .bind(id)
                    .execute(&mut *writer)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','A committed reply')")
                .bind(&slug).bind(id).execute(&mut *writer).await.unwrap();
                writer.commit().await.unwrap();
                resume.notify_one();
            };
            let (during, ()) = tokio::join!(get(if api_only { &api } else { &web }, &path), commit);
            assert!(!first.load(Ordering::SeqCst));
            public.close().await;
            assert!(
                !release_failed.load(Ordering::SeqCst),
                "connection-release barrier timed out"
            );
            let after = get(control_app, &path).await;
            assert_ne!(before, after, "the committed control must change {path}");
            if during != before && during != after {
                inconsistent.push(format!(
                    "table_lock={table_lock}, api_only={api_only}, json={json}"
                ));
            }
        }
    }
    assert!(
        inconsistent.is_empty(),
        "thread responses combined different commits: {inconsistent:?}"
    );
    settled_contracts(&owner, &control, &slug, id).await;
}

async fn settled_contracts(owner: &PgPool, public: &PgPool, slug: &str, id: i64) {
    let snapshot = board_store::thread_snapshot(public, slug, id)
        .await
        .unwrap();
    assert_eq!(snapshot.board.title, "After commit");
    assert_eq!(snapshot.thread.reply_count, 1);
    assert_eq!(snapshot.posts.len(), 2);
    assert_eq!(snapshot.posts[0].id, id);
    assert!(snapshot.posts[1].id > id);
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id<>$2")
        .bind(slug)
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    let snapshot = board_store::thread_snapshot(public, slug, id)
        .await
        .unwrap();
    assert_eq!(snapshot.posts.len(), 1);
    assert_eq!(
        snapshot.thread.reply_count, 1,
        "the lifetime count is not the visible count"
    );
    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let path = format!("/{slug}/thread/{id}.json");
    let mut validators = Vec::new();
    for app in [&web, &api] {
        let response = request(app, &path, None).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=0, must-revalidate"
        );
        assert_eq!(
            response.headers()["last-modified"],
            "Fri, 02 Jan 2026 00:00:00 GMT"
        );
        let etag = response.headers()["etag"].to_str().unwrap().to_owned();
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["posts"].as_array().unwrap().len(), 1);
        assert_eq!(body["posts"][0]["replies"], 0);
        assert_eq!(body["posts"][0]["bumplimit"], 1);
        let cached = request(app, &path, Some(&etag)).await;
        assert_eq!(cached.status(), 304);
        assert!(
            cached
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .is_empty()
        );
        validators.push((app, etag));
    }
    sqlx::query("UPDATE content.boards SET bump_limit=50 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    for (app, old_etag) in validators {
        let changed = request(app, &path, Some(&old_etag)).await;
        assert_eq!(
            changed.status(),
            200,
            "board-only changes invalidate the body ETag"
        );
        assert_ne!(changed.headers()["etag"], old_etag);
        let current = changed.headers()["etag"].to_str().unwrap();
        assert_eq!(request(app, &path, Some(current)).await.status(), 304);
    }
    for deleted in [false, true] {
        if deleted {
            sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
                .bind(id)
                .execute(owner)
                .await
                .unwrap();
        }
        for (app, json) in [(&web, false), (&web, true), (&api, true)] {
            let suffix = if json { ".json" } else { "" };
            let key = if deleted {
                id.to_string()
            } else {
                i64::MAX.to_string()
            };
            let missing = request(app, &format!("/{slug}/thread/{key}{suffix}"), None).await;
            assert_eq!(missing.status(), 404);
            assert_eq!(missing.headers()["cache-control"], "no-store");
            let body = missing.into_body().collect().await.unwrap().to_bytes();
            assert!(
                String::from_utf8(body.to_vec())
                    .unwrap()
                    .contains("Board, thread, or post not found.")
            );
        }
    }
    for invalid in ["bad!", "", "waytoolongboardname"] {
        assert!(matches!(
            board_store::thread_snapshot(public, invalid, id).await,
            Err(board_store::StoreError::NotFound)
        ));
    }
}

#[tokio::test]
async fn thread_representations_include_board_settings_in_their_snapshot() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let control = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut setup = owner.begin().await.unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Before commit','Owned thread snapshot fixture',1000,100,50,10,10)")
        .bind(&slug).execute(&mut *setup).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&slug)
        .fetch_one(&mut *setup)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Snapshot','Before commit')")
        .bind(id).bind(&slug).execute(&mut *setup).await.unwrap();
    setup.commit().await.unwrap();

    let result = tokio::spawn(coherent_during_commit(
        owner.clone(),
        control.clone(),
        slug.clone(),
        id,
    ))
    .await;
    control.close().await;
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
