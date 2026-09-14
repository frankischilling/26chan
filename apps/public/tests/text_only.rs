#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use board_store::{NewPost, StoreError};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

fn post(subject: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: subject.into(),
        comment: "Owned required-subject post".into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

async fn snapshot(owner: &PgPool, slug: &str) -> String {
    sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT count(*) FROM content.posts WHERE board=$1),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=$1),'sequence',(SELECT last_value FROM content.post_number))::text")
        .bind(slug).fetch_one(owner).await.unwrap()
}

async fn submit(
    app: &Router,
    slug: &str,
    index: usize,
    parent: i64,
    subject: &str,
    comment: &str,
) -> axum::response::Response {
    let fields = [
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("sub", subject.to_owned()),
        ("com", comment.to_owned()),
        ("pwd", "owned-password".to_owned()),
    ];
    let (kind, body) = if index & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!("--owned-required\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-required--\r\n");
        ("multipart/form-data; boundary=owned-required", body)
    };
    let route = if index & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    let accept = if index & 4 == 0 {
        "application/json"
    } else {
        "text/html"
    };
    app.clone()
        .oneshot(
            Request::post(format!("/{slug}/{route}"))
                .header("origin", "http://127.0.0.1:3000")
                .header("accept", accept)
                .header("content-type", kind)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn accepted_id(response: axum::response::Response, index: usize) -> i64 {
    if index & 4 == 0 {
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert!(json.get("error").is_none(), "{json}");
        json["pid"].as_i64().unwrap()
    } else {
        assert_eq!(response.status(), 303);
        response.headers()["location"]
            .to_str()
            .unwrap()
            .split("#p")
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    sqlx::query(
        "UPDATE content.boards SET image_limit=3,comment_spoiler_cleanup=true WHERE slug=$1",
    )
    .bind(&slug)
    .execute(&owner)
    .await
    .unwrap();
    let historical = board_store::create_post(&public, &slug, 0, &post(""))
        .await
        .unwrap();
    // These routes only read media settings; intake transport is exercised by
    // the real streaming test, not this metadata-only configured listener.
    let media = board_config::PublicMediaSettings::development(
        "127.0.0.1:1",
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    let (app, api) = board_public::routers_with_media(
        public.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        Some(media),
    );
    for enabled in [false, true] {
        sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&owner)
            .await
            .unwrap();
        for router in [&app, &api] {
            let response = router
                .clone()
                .oneshot(Request::get("/boards.json").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let json: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                    .unwrap();
            let board = json["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["board"] == slug)
                .unwrap();
            for field in ["text_only", "require_subject"] {
                if enabled {
                    assert_eq!(board[field].as_i64(), Some(1));
                } else {
                    assert!(board.get(field).is_none());
                }
            }
        }
        for path in [format!("/{slug}/"), format!("/{slug}/thread/{historical}")] {
            let response = app
                .clone()
                .oneshot(Request::get(&path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let html = String::from_utf8(
                to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert_eq!(html.contains("<body class=\"text_only\">"), enabled);
            assert_eq!(html.contains("Upload file</button>"), !enabled);
            if path.ends_with('/') {
                assert_eq!(
                    html.contains("id=\"sub\" name=\"sub\" type=\"text\" required"),
                    enabled
                );
            }
        }
    }
    let denied = sqlx::query("UPDATE content.boards SET text_only=false WHERE slug=$1")
        .bind(&slug)
        .execute(&public)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    for index in 0..8 {
        let before = snapshot(&owner, &slug).await;
        let response = submit(&app, &slug, index, 0, "##😀", "Owned comment").await;
        assert_eq!(response.status(), if index & 4 == 0 { 200 } else { 422 });
        let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
        if index & 4 == 0 {
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["error"], "Error: New threads require a subject.");
        } else {
            assert!(
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .contains("Error: New threads require a subject.")
            );
        }
        assert_eq!(snapshot(&owner, &slug).await, before);
        let op = accepted_id(
            submit(&app, &slug, index, 0, "Owned subject", "").await,
            index,
        )
        .await;
        let reply = accepted_id(
            submit(&app, &slug, index, op, "", "Owned reply").await,
            index,
        )
        .await;
        assert_eq!(
            board_store::find_post(&public, &slug, reply)
                .await
                .unwrap()
                .comment,
            "Owned reply"
        );
    }
    // Lock witnesses distinguish committed policy from stale handler state.
    for enabled in [false, true] {
        let before = snapshot(&owner, &slug).await;
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&mut *locked)
            .await
            .unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(
                async move { board_store::create_post(&public, &slug, 0, &post("")).await },
            )
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async { loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(pid).fetch_one(&owner).await.unwrap();
            if waiting { break; } tokio::time::sleep(Duration::from_millis(10)).await;
        }}).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("posting waited on text-only board policy");
        if enabled {
            assert!(matches!(
                result,
                Err(StoreError::Invalid("Error: New threads require a subject."))
            ));
            assert_eq!(snapshot(&owner, &slug).await, before);
        } else {
            assert!(result.is_ok());
        }
    }
    assert!(
        board_store::find_post(&public, &slug, historical)
            .await
            .unwrap()
            .subject
            .is_empty()
    );
}
#[tokio::test]
async fn text_only_policy_controls_http_json_forms_and_locked_admission() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Final content admission','Owned fixture',1000,100,100,100,10)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
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
