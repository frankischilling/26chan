#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn submit(
    app: &axum::Router,
    path: &str,
    parent: i64,
    option: &str,
    comment: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "resto={parent}&email={option}&com={comment}&password=synthetic-password-123"
                )))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut failures = Vec::new();
    let mut first_thread = None;
    for (alias_index, alias) in ["post", "imgboard.php"].into_iter().enumerate() {
        let path = format!("/{slug}/{alias}");
        for (option, sage, board_return) in [
            ("", false, false),
            ("sage", true, false),
            ("nonoko", false, true),
            ("nonokosage", true, true),
        ] {
            let comment = format!("owned-{alias_index}-{option}-op");
            let response = submit(&app, &path, 0, option, &comment).await;
            if response.status() != 303 {
                failures.push(format!("{alias} {option}: {}", response.status()));
                continue;
            }
            let id: i64 =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND comment=$2")
                    .bind(&slug)
                    .bind(&comment)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            first_thread.get_or_insert(id);
            let expected = if board_return {
                format!("/{slug}/")
            } else {
                format!("/{slug}/thread/{id}#p{id}")
            };
            assert_eq!(response.headers()["location"], expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let post = board_store::find_post(&public, &slug, id).await.unwrap();
            assert_eq!(post.thread_id, id);
            assert_eq!(post.name, "Anonymous");
            // A fixed old timestamp proves bump suppression without sleeps.
            sqlx::query("UPDATE content.threads SET bumped_at='2026-01-01T00:00:00Z' WHERE id=$1")
                .bind(id)
                .execute(&owner)
                .await
                .unwrap();
            let before = board_store::thread(&public, &slug, id).await.unwrap();
            let reply_comment = format!("owned-{alias_index}-{option}-reply");
            let reply = submit(&app, &path, id, option, &reply_comment).await;
            assert_eq!(reply.status(), 303, "{alias} {option}");
            let reply_id: i64 =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND comment=$2")
                    .bind(&slug)
                    .bind(&reply_comment)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            let expected = if board_return {
                format!("/{slug}/")
            } else {
                format!("/{slug}/thread/{id}#p{reply_id}")
            };
            assert_eq!(reply.headers()["location"], expected);
            let after = board_store::thread(&public, &slug, id).await.unwrap();
            assert_eq!(after.reply_count, 1);
            assert_eq!(
                after.bumped_at == before.bumped_at,
                sage,
                "{alias} {option}"
            );
            if !sage {
                assert!(after.bumped_at > before.bumped_at);
            }
            let posts = board_store::posts(&public, &slug, id).await.unwrap();
            assert_eq!(posts.len(), 2);
            assert_eq!(posts[1].id, reply_id);
        }
        for invalid in ["NONOKO", "nonoko%20sage", "nonokononokosage"] {
            let before: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&slug)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            let response = submit(&app, &path, 0, invalid, "invalid-option-must-not-persist").await;
            assert_eq!(response.status(), 422);
            assert!(!response.headers().contains_key("location"));
            let after: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&slug)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_eq!(before, after);
        }
    }
    assert!(
        failures.is_empty(),
        "documented options rejected: {failures:?}"
    );
    let id = first_thread.unwrap();
    sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    for alias in ["post", "imgboard.php"] {
        let response = submit(
            &app,
            &format!("/{slug}/{alias}"),
            id,
            "nonokosage",
            "closed-thread-must-not-persist",
        )
        .await;
        assert_eq!(response.status(), 409);
        assert!(!response.headers().contains_key("location"));
    }
    assert_eq!(
        board_store::posts(&public, &slug, id).await.unwrap().len(),
        2
    );
}

#[tokio::test]
async fn documented_options_preserve_posts_redirects_and_bump_rules() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Posting options','Owned posting fixture',1000,100,50,20,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&slug).execute(&owner).await.unwrap();
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
