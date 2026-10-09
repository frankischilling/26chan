#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_store::NewPost;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str, etag: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(path);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn json_response(app: &Router, path: &str, etag: Option<&str>) -> (Value, String) {
    let response = get(app, path, etag).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()["cache-control"],
        "public, max-age=0, must-revalidate"
    );
    let current = response.headers()["etag"].to_str().unwrap().to_owned();
    if let Some(previous) = etag {
        assert_ne!(current, previous, "changed representation at {path}");
    }
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    (value, current)
}

fn op(value: &Value, endpoint: usize) -> &Value {
    match endpoint {
        0 | 1 => &value["posts"][0],
        2 => &value["threads"][0]["posts"][0],
        3 => &value[0]["threads"][0],
        _ => unreachable!(),
    }
}

fn replies(value: &Value, endpoint: usize) -> Vec<&Value> {
    match endpoint {
        0 | 1 => value["posts"].as_array().unwrap().iter().skip(1).collect(),
        2 => value["threads"][0]["posts"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .collect(),
        3 => value[0]["threads"][0]["last_replies"]
            .as_array()
            .unwrap()
            .iter()
            .collect(),
        _ => unreachable!(),
    }
}

async fn projections(
    apps: &[Router; 2],
    paths: &[String; 4],
    cap: Option<i32>,
    expected_replies: &[i64],
    previous: Option<&[String]>,
) -> Vec<String> {
    let mut etags = Vec::new();
    for app in apps {
        for (endpoint, path) in paths.iter().enumerate() {
            let prior = previous.map(|values| values[etags.len()].as_str());
            let (value, etag) = json_response(app, path, prior).await;
            let original = op(&value, endpoint);
            assert_eq!(original["replies"], json!(expected_replies.len()), "{path}");
            match cap {
                Some(cap) => assert_eq!(original["sticky_cap"], json!(cap), "{path}"),
                None => assert!(original.get("sticky_cap").is_none(), "{path}"),
            }
            let visible = replies(&value, endpoint);
            let expected = if endpoint == 1 {
                let tail_size = if cap.is_some() { 2 } else { 1 };
                assert_eq!(original["tail_size"], json!(tail_size), "{path}");
                &expected_replies[expected_replies.len().saturating_sub(tail_size)..]
            } else if endpoint >= 2 && original["sticky"] == 1 {
                &expected_replies[expected_replies.len().saturating_sub(1)..]
            } else if endpoint >= 2 {
                &expected_replies[expected_replies.len().saturating_sub(5)..]
            } else {
                expected_replies
            };
            assert_eq!(
                visible
                    .iter()
                    .map(|reply| reply["no"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                expected,
                "{path} excludes retired replies"
            );
            for reply in visible {
                assert!(reply.get("sticky_cap").is_none(), "OP-only field at {path}");
            }
            assert!(!value.to_string().contains("\"undead\""), "{path}");
            etags.push(etag);
        }
    }
    etags
}

async fn create(public: &PgPool, board: &str, parent: i64, comment: &str) -> i64 {
    posting::create_post(
        public,
        board,
        parent,
        &NewPost {
            name: "Anonymous".into(),
            subject: if parent == 0 {
                "Owned sticky API thread".into()
            } else {
                String::new()
            },
            comment: comment.into(),
            deletion_hash: "owned-sticky-api-hash".into(),
            sage: false,
        },
    )
    .await
    .unwrap()
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let parent = create(public, slug, 0, "Owned sticky API thread").await;
    let mut ids = Vec::new();
    for comment in [
        "Oldest reply",
        "Second reply",
        "Third reply",
        "Fourth reply",
        "Fifth reply",
        "Sixth reply",
    ] {
        ids.push(create(public, slug, parent, comment).await);
    }
    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let apps = [web, api];
    let paths = [
        format!("/{slug}/thread/{parent}.json"),
        format!("/{slug}/thread/{parent}-tail.json"),
        format!("/{slug}/1.json"),
        format!("/{slug}/catalog.json"),
    ];
    let mut previous = None;
    for (sticky, undead, cap) in [
        (false, false, None),
        (true, false, None),
        (true, true, Some(6)),
        (false, true, None),
        (true, true, Some(6)),
    ] {
        sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3 WHERE id=$1")
            .bind(parent)
            .bind(sticky)
            .bind(undead)
            .execute(owner)
            .await
            .unwrap();
        previous = Some(projections(&apps, &paths, cap, &ids, previous.as_deref()).await);
    }
    let newest = create(public, slug, parent, "Seventh reply rotates the window").await;
    ids.remove(0);
    ids.push(newest);
    previous = Some(projections(&apps, &paths, Some(6), &ids, previous.as_deref()).await);

    sqlx::query("UPDATE content.boards SET reply_limit=4,bump_limit=4 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let last = create(public, slug, parent, "Reduced window keeps the newest IDs").await;
    ids.drain(..ids.len() - 3);
    ids.push(last);
    let etags = projections(&apps, &paths, Some(4), &ids, previous.as_deref()).await;
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT reply_count FROM content.threads WHERE id=$1")
            .bind(parent)
            .fetch_one(owner)
            .await
            .unwrap(),
        4
    );
    for (index, app) in apps.iter().enumerate() {
        for (endpoint, path) in paths.iter().enumerate() {
            let cached = get(app, path, Some(&etags[index * paths.len() + endpoint])).await;
            assert_eq!(cached.status(), StatusCode::NOT_MODIFIED, "{path}");
            assert!(to_bytes(cached.into_body(), 8192).await.unwrap().is_empty());
        }
    }
    for condition in [
        "UPDATE content.boards SET staff_only=true WHERE slug=$1",
        "UPDATE content.boards SET staff_only=false,json_enabled=false WHERE slug=$1",
    ] {
        sqlx::query(condition)
            .bind(slug)
            .execute(owner)
            .await
            .unwrap();
        for app in &apps {
            for path in &paths {
                assert_eq!(
                    get(app, path, None).await.status(),
                    StatusCode::NOT_FOUND,
                    "{path}"
                );
            }
        }
    }
    sqlx::query("UPDATE content.boards SET json_enabled=true WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(parent)
        .execute(owner)
        .await
        .unwrap();
    for app in &apps {
        for path in &paths[..2] {
            assert_eq!(get(app, path, None).await.status(), StatusCode::NOT_FOUND);
        }
        for path in &paths[2..] {
            let (value, _) = json_response(app, path, None).await;
            assert!(!value.to_string().contains("sticky_cap"));
            assert!(!value.to_string().contains(&parent.to_string()));
        }
    }
}

#[tokio::test]
async fn sticky_cap_matches_enforced_retention_across_both_json_listeners() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(role, "board_public");
    let slug: String =
        sqlx::query_scalar("SELECT 'sc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,replies_shown,json_tail_size,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,text_only) VALUES($1,'Owned sticky API','Synthetic retention fixture',1000,6,6,5,5,5,1,0,0,0,true)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result = tokio::spawn(async move { exercise(&owned, &runtime, &board).await }).await;
    posting::cleanup_posting(&owner, &slug).await;
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    public.close().await;
    owner.close().await;
    result.unwrap();
}
