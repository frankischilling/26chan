#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use board_store::{NewPost, Thread};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

fn post(sage: bool) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned bump fixture".into(),
        deletion_hash: "synthetic-unused-hash".into(),
        sage,
    }
}

async fn reset_clock(owner: &PgPool, id: i64) {
    sqlx::query("UPDATE content.threads SET bumped_at='2026-01-01T00:00:00Z' WHERE id=$1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
}

async fn append(
    owner: &PgPool,
    public: &PgPool,
    slug: &str,
    id: i64,
    sage: bool,
    bump: bool,
) -> i64 {
    reset_clock(owner, id).await;
    let before = board_store::thread(public, slug, id).await.unwrap();
    let reply = board_store::create_post(public, slug, id, &post(sage))
        .await
        .unwrap();
    let after = board_store::thread(public, slug, id).await.unwrap();
    assert_eq!(after.bumped_at > before.bumped_at, bump);
    assert_eq!(after.reply_count, before.reply_count + 1);
    reply
}

async fn flags(app: &axum::Router, slug: &str, id: i64, replies: i64, limited: bool) -> String {
    let mut thread_etag = String::new();
    for (suffix, kind) in [
        (format!("thread/{id}.json"), 0),
        (format!("thread/{id}-tail.json"), 1),
        ("1.json".into(), 2),
        ("catalog.json".into(), 3),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/{slug}/{suffix}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if kind == 1 && replies <= 1 {
            assert_eq!(
                response.status(),
                404,
                "tail needs more than one surviving reply"
            );
            continue;
        }
        assert_eq!(response.status(), 200, "{suffix}");
        if kind == 0 {
            thread_etag = response.headers()["etag"].to_str().unwrap().into();
        }
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let op = match kind {
            0 | 1 => &value["posts"][0],
            2 => &value["threads"][0]["posts"][0],
            _ => &value[0]["threads"][0],
        };
        assert_eq!(op["replies"], replies, "{suffix}");
        assert_eq!(op["no"], id, "{suffix}");
        if kind == 1 || limited {
            assert_eq!(
                op["bumplimit"].as_i64(),
                Some(i64::from(limited)),
                "{suffix}"
            );
        } else {
            assert!(op.get("bumplimit").is_none(), "{suffix}");
        }
    }
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/{slug}/catalog"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
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
    let meta = html
        .split(&format!("id=\"meta-{id}\""))
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    assert_eq!(meta.contains("<i>R: <b>"), limited);
    thread_etag
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let id = board_store::create_post(&public, &slug, 0, &post(false))
        .await
        .unwrap();
    let first = append(&owner, &public, &slug, id, false, true).await;
    flags(&app, &slug, id, 1, false).await;
    let second = append(&owner, &public, &slug, id, false, true).await;
    flags(&app, &slug, id, 2, false).await;
    append(&owner, &public, &slug, id, false, false).await;
    let before = flags(&app, &slug, id, 3, true).await;
    board_store::delete_post(&public, &slug, first)
        .await
        .unwrap();
    board_store::delete_post(&public, &slug, second)
        .await
        .unwrap();
    assert_ne!(flags(&app, &slug, id, 1, false).await, before);
    append(&owner, &public, &slug, id, false, true).await;
    append(&owner, &public, &slug, id, true, false).await;
    flags(&app, &slug, id, 3, true).await;
    // Sticky suppression applies both below and above the configured limit.
    sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET bump_limit=100 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    append(&owner, &public, &slug, id, false, false).await;
    flags(&app, &slug, id, 4, false).await;
    sqlx::query("UPDATE content.boards SET bump_limit=0 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    flags(&app, &slug, id, 4, false).await;
    sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    flags(&app, &slug, id, 4, true).await;
    board_store::delete_post(&public, &slug, id).await.unwrap();

    // With a one-reply cutoff even the first concurrent reply must not bump.
    sqlx::query("UPDATE content.boards SET bump_limit=1 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let id = board_store::create_post(&public, &slug, 0, &post(false))
        .await
        .unwrap();
    reset_clock(&owner, id).await;
    let before: Thread = board_store::thread(&public, &slug, id).await.unwrap();
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let public = public.clone();
        let slug = slug.clone();
        jobs.spawn(async move {
            board_store::create_post(&public, &slug, id, &post(false))
                .await
                .unwrap()
        });
    }
    while let Some(result) = jobs.join_next().await {
        result.unwrap();
    }
    let after = board_store::thread(&public, &slug, id).await.unwrap();
    assert_eq!(after.bumped_at, before.bumped_at);
    assert_eq!(after.reply_count, 8);
    flags(&app, &slug, id, 8, true).await;
}

#[tokio::test]
async fn current_reply_counts_drive_bumps_and_all_public_limit_flags() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES($1,'Bump rules','Owned source-rule fixture',1000,100,3,10,10,1)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
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
