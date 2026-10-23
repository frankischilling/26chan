#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
async fn get(web: &Router, path: &str) -> Vec<u8> {
    let response = web
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    to_bytes(response.into_body(), 4_194_304)
        .await
        .unwrap()
        .to_vec()
}
#[tokio::test]
async fn imported_stock_identity_keeps_html_and_json_contracts_separate() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'sb'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned stock identity','Synthetic',1000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let op: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,since4pass) VALUES($1,$2,$1,'Anonymous','Stored stock identities','Synthetic',10000)").bind(op).bind(&board).execute(&owner).await.unwrap();
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let board = board.clone();
        tokio::spawn(async move {
            for id in 1i16..=231 {
                let glyph: String = sqlx::query_scalar("SELECT content.event_perk_text($1)")
                    .bind(id)
                    .fetch_one(&public)
                    .await
                    .unwrap();
                assert_eq!(
                    glyph,
                    board_domain::source_events::perks::from_stored(id)
                        .unwrap()
                        .glyph()
                );
            }
            for value in 10001..=10017 {
                sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment,since4pass) VALUES($1,$2,'Anonymous','','Synthetic',$3)")
                .bind(&board).bind(op).bind(value).execute(&owner).await.unwrap();
            }
            let pass:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment,since4pass) VALUES($1,$2,'Anonymous','','Owned Pass presentation',2020) RETURNING id")
                .bind(&board).bind(op).fetch_one(&owner).await.unwrap();
            sqlx::query("UPDATE content.posts SET event_perk=CASE WHEN id=$1 THEN 231 ELSE (since4pass-9999)::smallint END WHERE board=$2").bind(pass).bind(&board).execute(&owner).await.unwrap();
            for invalid in [-1, 10018, i32::MAX] {
                let error = sqlx::query("UPDATE content.posts SET since4pass=$1 WHERE id=$2")
                    .bind(invalid)
                    .bind(op)
                    .execute(&owner)
                    .await
                    .unwrap_err();
                assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("23514")
                );
            }
            assert!(
                sqlx::query("UPDATE content.posts SET since4pass=10017 WHERE id=$1")
                    .bind(op)
                    .execute(&public)
                    .await
                    .is_err()
            );
            let glyph = board_domain::source_events::perks::from_stored(231)
                .unwrap()
                .glyph();
            let found = board_store::search(&public, &glyph, Some(&board), 0)
                .await
                .unwrap();
            assert_eq!(found.nhits, 1);
            assert!(
                found.threads[0]
                    .posts
                    .iter()
                    .any(|p| p.id == pass && p.event_perk == Some(231))
            );
            let board_row = board_store::board(&public, &board).await.unwrap();
            let saved = board_store::find_post(&public, &board, pass).await.unwrap();
            let teaser = board_public::views::PostView::new(saved).catalog_teaser(&board_row);
            assert!(teaser.serialized.contains(&glyph));
            let web = board_public::router(public.clone(), "http://localhost:3000".into(), false);
            let html =
                String::from_utf8(get(&web, &format!("/{board}/thread/{op}")).await).unwrap();
            for symbol in board_domain::source_events::STOCKS {
                assert_eq!(
                    html.matches(&format!("class=\"n-xa24 n-xa24-{symbol}\""))
                        .count(),
                    2,
                    "{symbol}"
                );
                assert_eq!(
                    html.matches(&format!(" p-xa24-{symbol}\"")).count(),
                    1,
                    "{symbol}"
                );
            }
            assert!(!html.contains("Pass user since 100"));
            assert_eq!(html.matches("class=\"like-perk-cnt\"").count(), 19);
            let json: Value =
                serde_json::from_slice(&get(&web, &format!("/{board}/thread/{op}.json")).await)
                    .unwrap();
            assert_eq!(json["posts"].as_array().unwrap().len(), 19);
            for post in json["posts"].as_array().unwrap() {
                let saved: i16 =
                    sqlx::query_scalar("SELECT event_perk FROM content.posts WHERE id=$1")
                        .bind(post["no"].as_i64().unwrap())
                        .fetch_one(&owner)
                        .await
                        .unwrap();
                assert!(
                    post["com"].as_str().unwrap().contains(
                        &board_domain::source_events::perks::from_stored(saved)
                            .unwrap()
                            .source_html()
                    )
                );
                if post["no"].as_i64() == Some(pass) {
                    assert_eq!(post["since4pass"], 2020);
                } else {
                    assert!(post.get("since4pass").is_none());
                }
            }
            assert_eq!(html.matches("class=\"n-pu\"").count(), 2);
            let snapshot: Value = serde_json::from_slice(
                &get(&web, &format!("/_watch/{board}/thread/{op}/posts")).await,
            )
            .unwrap();
            let context = serde_json::json!({"origin":"http://localhost:3000","board":board,"thread":op.to_string()});
            let mut command = tokio::process::Command::new("node");
            command.env_clear();
            for name in ["PATH", "HOME", "SystemRoot", "TEMP", "TMP"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            use std::process::Stdio;
            use tokio::io::AsyncWriteExt;
            let mut child = command
                .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                .arg("tests/browser/event-stock-snapshot.mjs")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            let input =
                serde_json::to_vec(&serde_json::json!({"snapshot":snapshot,"context":context}))
                    .unwrap();
            child.stdin.take().unwrap().write_all(&input).await.unwrap();
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                    .await
                    .unwrap()
                    .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            public.close().await;
        })
    };
    let outcome = work.await;
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
    owner.close().await;
    outcome.unwrap();
}
