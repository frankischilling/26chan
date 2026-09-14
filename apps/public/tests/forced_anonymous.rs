#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use board_store::NewPost;
use rand_core::{OsRng, RngCore};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
use tower::ServiceExt;

async fn submit(
    app: &Router,
    slug: &str,
    index: usize,
    parent: i64,
    comment: &str,
) -> axum::response::Response {
    let fields = [
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("name", "Owned <name>#trip".to_owned()),
        ("sub", "Owned subject".to_owned()),
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
            body.push_str(&format!(
                "--owned-anon\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned-anon--\r\n");
        ("multipart/form-data; boundary=owned-anon", body)
    };
    let route = if index & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    app.clone()
        .oneshot(
            Request::post(format!("/{slug}/{route}"))
                .header("origin", "http://127.0.0.1:3000")
                .header(
                    "accept",
                    if index & 4 == 0 {
                        "application/json"
                    } else {
                        "text/html"
                    },
                )
                .header("content-type", kind)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn accepted(response: axum::response::Response, index: usize) -> i64 {
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

async fn get(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn body(response: axum::response::Response) -> String {
    assert_eq!(response.status(), 200);
    String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn error(response: axum::response::Response, expected: &str) {
    assert_eq!(response.status(), 200);
    let value: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(value["error"], expected);
}
fn draft() -> NewPost {
    NewPost {
        name: "Owned name".into(),
        subject: "Owned subject".into(),
        comment: "Owned comment".into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    // This matrix deliberately performs more writes than the production default.
    // The rate limiter is independently exercised by http_limits.rs.
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "1000".into())
    })
    .unwrap();
    let (_, app, api) = board_public::observed_routers_with_limits(
        public.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        true,
        None,
        limits,
    );
    let old = board_store::create_post(&public, &slug, 0, &draft())
        .await
        .unwrap();
    let mut previous_etag = None;
    for enabled in [false, true, false] {
        sqlx::query("UPDATE content.boards SET forced_anon=$2 WHERE slug=$1")
            .bind(&slug)
            .bind(enabled)
            .execute(&owner)
            .await
            .unwrap();
        for router in [&app, &api] {
            let response = get(router, "/boards.json").await;
            let etag = response.headers()["etag"].clone();
            let json: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
            let board = json["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["board"] == slug)
                .unwrap();
            if enabled {
                assert_eq!(board["forced_anon"].as_i64(), Some(1));
            } else {
                assert!(board.get("forced_anon").is_none());
            }
            let same = router
                .clone()
                .oneshot(
                    Request::get("/boards.json")
                        .header("if-none-match", &etag)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(same.status(), 304);
            assert!(to_bytes(same.into_body(), 1).await.unwrap().is_empty());
            if let Some(prior) = &previous_etag {
                let changed = router
                    .clone()
                    .oneshot(
                        Request::get("/boards.json")
                            .header("if-none-match", prior)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(changed.status(), 200);
            }
        }
        previous_etag = Some(get(&app, "/boards.json").await.headers()["etag"].clone());
        let html = body(get(&app, &format!("/{slug}/")).await).await;
        assert_eq!(html.contains("id=\"name\""), !enabled);
        assert_eq!(html.contains("id=\"sub\""), !enabled);
        assert_eq!(html.contains("type=\"hidden\" name=\"name\""), enabled);
        for index in 0..8 {
            let op = accepted(submit(&app, &slug, index, 0, "Owned comment").await, index).await;
            let reply = accepted(submit(&app, &slug, index, op, "Owned reply").await, index).await;
            for id in [op, reply] {
                let saved = board_store::find_post(&public, &slug, id).await.unwrap();
                assert_eq!(
                    saved.name,
                    if enabled {
                        "Anonymous"
                    } else {
                        "Owned <name>#trip"
                    }
                );
                assert_eq!(saved.subject, if enabled { "" } else { "Owned subject" });
            }
            for router in [&app, &api] {
                let json: serde_json::Value = serde_json::from_str(
                    &body(get(router, &format!("/{slug}/thread/{op}.json")).await).await,
                )
                .unwrap();
                assert_eq!(
                    json["posts"][0]["name"],
                    if enabled {
                        "Anonymous"
                    } else {
                        "Owned <name>#trip"
                    }
                );
                assert_eq!(json["posts"][0].get("sub").is_some(), !enabled);
            }
        }
        let historical = board_store::find_post(&public, &slug, old).await.unwrap();
        assert_eq!(historical.name, "Owned name");
        assert_eq!(historical.subject, "Owned subject");
    }
    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    error(
        submit(&app, &slug, 0, 0, "").await,
        "Error: New threads require a subject or comment.",
    )
    .await;
    sqlx::query("UPDATE content.boards SET require_subject=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    error(
        submit(&app, &slug, 0, 0, "Owned comment").await,
        "Error: New threads require a subject.",
    )
    .await;
    accepted(submit(&app, &slug, 0, old, "Owned reply").await, 0).await;
    sqlx::query("UPDATE content.boards SET require_subject=false,text_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    error(
        submit(&app, &slug, 0, 0, "Owned comment").await,
        "Error: New threads require a subject.",
    )
    .await;
    sqlx::query("UPDATE content.boards SET text_only=false,forced_anon=false WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    // Observe the real writer waiting behind this exact policy transaction.
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&public)
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(&slug)
        .execute(&mut *tx)
        .await
        .unwrap();
    let child_app = app.clone();
    let child_slug = slug.clone();
    let pending = tokio::spawn(async move {
        accepted(
            submit(&child_app, &child_slug, 0, 0, "Owned blocked post").await,
            0,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(&owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("writer did not reach the policy lock");
    tx.commit().await.unwrap();
    let id = pending.await.unwrap();
    let saved = board_store::find_post(&public, &slug, id).await.unwrap();
    assert_eq!(
        (saved.name.as_str(), saved.subject.as_str()),
        ("Anonymous", "")
    );
    for invalid in [
        "x".repeat(board_domain::MAX_PUBLIC_FIELD_BYTES + 1),
        "bad\0name".into(),
    ] {
        let post = NewPost {
            name: invalid,
            ..draft()
        };
        assert!(matches!(
            board_store::create_post(&public, &slug, 0, &post).await,
            Err(board_store::StoreError::Invalid(_))
        ));
    }
    let error = sqlx::query("UPDATE content.boards SET forced_anon=false WHERE slug=$1")
        .bind(&slug)
        .execute(&public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
}

#[tokio::test]
async fn forced_anonymous_clears_new_identity_before_admission_and_preserves_history() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Forced anonymous','Owned fixture',4000,200,150,100,10)").bind(&slug).execute(&owner).await.unwrap();
    let outcome = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
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
    outcome.unwrap();
}
