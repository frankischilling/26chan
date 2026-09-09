#![cfg(feature = "database-tests")]

use board_store::{NewPost, StoreError};
use sqlx::Executor;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_board() -> String {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
        % 0x10_0000;
    format!("l{suffix:06x}")
}

fn post(comment: String) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment,
        deletion_hash: "fixture-not-a-valid-password-hash".into(),
        sage: false,
    }
}

#[tokio::test]
async fn public_posts_use_board_character_limits_and_database_global_limits() {
    let public_url = std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap();
    let migration_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let public = board_store::connect_public(&public_url).await.unwrap();
    let migration = sqlx::PgPool::connect(&migration_url).await.unwrap();
    let slug = fixture_board();

    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Comment limits','Disposable limit fixture',4,100,100,100,10)")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();

    let thread = board_store::create_post(&public, &slug, 0, &post("é".repeat(4)))
        .await
        .unwrap();
    board_store::create_post(&public, &slug, thread, &post("😀".repeat(4)))
        .await
        .unwrap();
    board_store::create_post(&public, &slug, thread, &post("e\u{301}".repeat(2)))
        .await
        .unwrap();
    let before = board_store::visible_post_count(&public, &slug, thread)
        .await
        .unwrap();
    assert!(matches!(
        board_store::create_post(&public, &slug, thread, &post("é".repeat(5))).await,
        Err(StoreError::Invalid(_))
    ));
    assert_eq!(
        board_store::visible_post_count(&public, &slug, thread)
            .await
            .unwrap(),
        before
    );

    sqlx::query("UPDATE content.boards SET max_comment_chars=16000 WHERE slug=$1")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();
    let full_comment = "😀".repeat(16_000);
    let full_id = board_store::create_post(&public, &slug, thread, &post(full_comment.clone()))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, full_id)
            .await
            .unwrap()
            .comment,
        full_comment
    );

    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&public)
        .await
        .unwrap();
    let error = public
        .execute(
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$3,'Anonymous','',$4)")
                .bind(id)
                .bind(&slug)
                .bind(thread)
                .bind("x".repeat(16_001)),
        )
        .await
        .unwrap_err();
    let database_error = error.as_database_error().unwrap();
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(database_error.constraint(), Some("posts_comment_check"));

    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&migration)
        .await
        .unwrap();
    public.close().await;
    migration.close().await;
}
