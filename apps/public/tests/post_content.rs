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
    sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut thread = 0;
    // Both post routes, form encodings and response modes use final rendered
    // content. A subject admits an OP but never an unattached blank reply.
    for index in 0..8 {
        let id = accepted_id(
            submit(&app, &slug, index, 0, "Owned subject", "").await,
            index,
        )
        .await;
        thread = id;
        let saved = board_store::find_post(&public, &slug, id).await.unwrap();
        assert_eq!(saved.subject, "Owned subject");
        assert!(saved.comment.is_empty());
        let response = api
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/thread/{id}.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap();
        assert_eq!(json["posts"][0]["sub"], "Owned subject");
        assert!(json["posts"][0].get("com").is_none());
        for (parent, subject, error) in [
            (0, "", "Error: New threads require a subject or comment."),
            (id, "Reply subject", "Error: No text entered."),
        ] {
            let before = snapshot(&owner, &slug).await;
            let response = submit(
                &app,
                &slug,
                index,
                parent,
                subject,
                "[spoiler] \n[/spoiler]",
            )
            .await;
            assert_eq!(response.status(), if index & 4 == 0 { 200 } else { 422 });
            let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
            if index & 4 == 0 {
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(json["error"], error);
                assert!(json.get("pid").is_none());
            } else {
                assert!(std::str::from_utf8(&bytes).unwrap().contains(error));
            }
            assert_eq!(snapshot(&owner, &slug).await, before);
        }
    }
    for mask in 0..8 {
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2,comment_code_spacing=$3,comment_sjis_spacing=$4 WHERE slug=$1")
            .bind(&slug).bind(mask & 1 != 0).bind(mask & 2 != 0).bind(mask & 4 != 0)
            .execute(&owner).await.unwrap();
        for (raw, blank) in [
            ("[spoiler] \n[/spoiler]", mask & 1 != 0),
            ("[code]      [/code]", mask & 2 != 0),
            ("[code]       [/code]", false),
            ("[sjis][/sjis]", false),
        ] {
            for parent in [0, thread] {
                let before = snapshot(&owner, &slug).await;
                let draft = NewPost {
                    comment: raw.into(),
                    ..post("")
                };
                let result = board_store::create_post(&public, &slug, parent, &draft).await;
                if blank {
                    let expected = if parent == 0 {
                        "Error: New threads require a subject or comment."
                    } else {
                        "Error: No text entered."
                    };
                    assert!(matches!(result, Err(StoreError::Invalid(error)) if error == expected));
                    assert_eq!(snapshot(&owner, &slug).await, before);
                } else {
                    let saved = board_store::find_post(&public, &slug, result.unwrap())
                        .await
                        .unwrap();
                    assert_eq!(saved.comment_format, 40 + mask);
                }
            }
        }
    }
    // Witness an actual blocked insertion in both policy directions. Final
    // admission and the persisted stamp must use the policy after the lock.
    for enabled in [true, false] {
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(!enabled)
            .execute(&owner)
            .await
            .unwrap();
        let before = snapshot(&owner, &slug).await;
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&mut *locked)
            .await
            .unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(async move {
                board_store::create_post(
                    &public,
                    &slug,
                    thread,
                    &NewPost {
                        comment: "[spoiler][/spoiler]".into(),
                        ..post("")
                    },
                )
                .await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async { loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(pid).fetch_one(&owner).await.unwrap();
            if waiting { break; } tokio::time::sleep(Duration::from_millis(10)).await;
        }}).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("posting waited for final markup policy");
        if enabled {
            assert!(matches!(
                result,
                Err(StoreError::Invalid("Error: No text entered."))
            ));
            assert_eq!(snapshot(&owner, &slug).await, before);
        } else {
            assert_eq!(
                board_store::find_post(&public, &slug, result.unwrap())
                    .await
                    .unwrap()
                    .comment_format,
                46
            );
        }
    }
}
#[tokio::test]
async fn post_markup_admission_checks_http_transactions_and_locked_policy() {
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
