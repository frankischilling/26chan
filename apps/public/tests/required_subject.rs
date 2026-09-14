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
) -> axum::response::Response {
    let fields = [
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("sub", subject.to_owned()),
        ("com", "Owned required-subject HTTP post".to_owned()),
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
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    for router in [&app, &api] {
        let mut previous_etag = String::new();
        for enabled in [false, true, false] {
            sqlx::query("UPDATE content.boards SET require_subject=$2 WHERE slug=$1")
                .bind(&slug)
                .bind(enabled)
                .execute(&owner)
                .await
                .unwrap();
            let mut request = Request::get("/boards.json");
            if !previous_etag.is_empty() {
                request = request.header("if-none-match", &previous_etag);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                200,
                "Changed policy must invalidate the board validator"
            );
            let etag = response.headers()["etag"].to_str().unwrap().to_owned();
            assert_ne!(etag, previous_etag);
            let json: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                    .unwrap();
            let board = json["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|board| board["board"] == slug)
                .unwrap();
            if enabled {
                assert_eq!(board["require_subject"].as_i64(), Some(1));
            } else {
                assert!(board.get("require_subject").is_none());
            }
            let unchanged = router
                .clone()
                .oneshot(
                    Request::get("/boards.json")
                        .header("if-none-match", &etag)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(unchanged.status(), 304);
            assert!(
                to_bytes(unchanged.into_body(), 1024)
                    .await
                    .unwrap()
                    .is_empty()
            );
            previous_etag = etag;
        }
    }
    let historical = board_store::create_post(&public, &slug, 0, &post(""))
        .await
        .unwrap();
    assert!(
        !board_store::board(&public, &slug)
            .await
            .unwrap()
            .require_subject
    );
    sqlx::query("UPDATE content.boards SET require_subject=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let denied = sqlx::query("UPDATE content.boards SET require_subject=false WHERE slug=$1")
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
        let response = submit(&app, &slug, index, 0, "＃##😀").await;
        assert_eq!(response.status(), if index & 4 == 0 { 200 } else { 422 });
        let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
        if index & 4 == 0 {
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["error"], "Error: New threads require a subject.");
            assert!(json.get("pid").is_none());
        } else {
            assert!(
                std::str::from_utf8(&bytes)
                    .unwrap()
                    .contains("Error: New threads require a subject.")
            );
        }
        assert_eq!(snapshot(&owner, &slug).await, before);
        let op = accepted_id(submit(&app, &slug, index, 0, "Ｚ##ⓦ <b>").await, index).await;
        assert_eq!(
            board_store::find_post(&public, &slug, op)
                .await
                .unwrap()
                .subject,
            "aw <b>"
        );
        let reply = accepted_id(submit(&app, &slug, index, op, "##😀").await, index).await;
        assert!(
            board_store::find_post(&public, &slug, reply)
                .await
                .unwrap()
                .subject
                .is_empty()
        );
    }
    // Observe the actual row-lock wait in both policy directions. The committed
    // setting, not an earlier read or a slug-specific branch, decides admission.
    for enabled in [false, true] {
        let before = snapshot(&owner, &slug).await;
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET require_subject=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&mut *locked)
            .await
            .unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(
                async move { board_store::create_post(&public, &slug, 0, &post("##")).await },
            )
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async { loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))").bind(pid).fetch_one(&owner).await.unwrap();
            if waiting { break; } tokio::time::sleep(Duration::from_millis(10)).await;
        } }).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("posting waited for required-subject policy");
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
    let before = snapshot(&owner, &slug).await;
    for (subject, comment, error) in [
        ("#".repeat(101), "ok".into(), "Name or subject is too long."),
        (
            "".into(),
            "x".repeat(1001),
            "Enter a comment within this board's character limit.",
        ),
        (
            "".into(),
            format!("{}end", "x\n".repeat(7)),
            "Error: New threads require a subject.",
        ),
        (
            "".into(),
            "".into(),
            "Error: New threads require a subject.",
        ),
    ] {
        let mut draft = post(&subject);
        draft.comment = comment;
        assert!(
            matches!(board_store::create_post(&public, &slug, 0, &draft).await, Err(StoreError::Invalid(message)) if message == error)
        );
    }
    assert_eq!(snapshot(&owner, &slug).await, before);
    assert!(
        board_store::find_post(&public, &slug, historical)
            .await
            .unwrap()
            .subject
            .is_empty()
    );
}

#[tokio::test]
async fn required_subject_is_locked_operator_policy_for_new_threads_only() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Required subject','Owned fixture',1000,100,100,100,10)").bind(&slug).execute(&owner).await.unwrap();
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
