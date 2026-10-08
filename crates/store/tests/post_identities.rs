#![cfg(feature = "database-tests")]
mod support;

use board_store::{NewPost, PostingContext, StoreError};
use sqlx::postgres::PgPoolOptions;

fn post(name: &str) -> NewPost {
    NewPost {
        name: name.into(),
        subject: "Owned identity".into(),
        comment: "Owned comment".into(),
        deletion_hash: "owned-synthetic-hash".into(),
        sage: false,
    }
}

#[tokio::test]
async fn posting_identity_is_transaction_local_and_failed_identity_rolls_back() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // One real runtime connection makes reuse deterministic.
    let public = PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned identity','Owned fixture',1000,100,100,100,10,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let thread = support::create_post(&public, &board, 0, &post("User#password"))
        .await
        .unwrap();
    let plain = support::create_post(&public, &board, thread, &post("Plain name"))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, thread)
            .await
            .unwrap()
            .trip
            .as_deref(),
        Some("!ozOtJW9BFA")
    );
    assert_eq!(
        board_store::find_post(&public, &board, plain)
            .await
            .unwrap()
            .trip,
        None
    );
    let setting: Option<String> =
        sqlx::query_scalar("SELECT nullif(current_setting('board.post_trip',true),'')")
            .fetch_one(&public)
            .await
            .unwrap();
    assert_eq!(setting, None);
    let before = board_store::thread(&public, &board, thread).await.unwrap();
    assert!(matches!(
        support::create_post(&public, &board, thread, &post("User##private-password")).await,
        Err(StoreError::Invalid("Secure tripcodes are unavailable."))
    ));
    let after = board_store::thread(&public, &board, thread).await.unwrap();
    assert_eq!(before.reply_count, after.reply_count);
    assert_eq!(before.modified_at, after.modified_at);
    let key = board_domain::identity::SecureKey::parse(&"1".repeat(64)).unwrap();
    let secure = support::create_post_with_context_and_key(
        &public,
        &board,
        thread,
        &post("User##password"),
        None,
        PostingContext {
            request_start: chrono::Utc::now(),
            peer: None,
            op_password_proof: None,
        },
        Some(&key),
    )
    .await
    .unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, secure)
            .await
            .unwrap()
            .trip
            .as_deref(),
        Some("!!XYWOFgjf7hP")
    );
    let later = support::create_post(&public, &board, thread, &post("Later name"))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &board, later)
            .await
            .unwrap()
            .trip,
        None
    );
    // Identical pseudonyms never replace deletion authority or staff credentials.
    let hashes: Vec<String> =
        sqlx::query_scalar("SELECT password_hash FROM post_secrets.deletion WHERE post_id=ANY($1)")
            .bind(vec![thread, secure])
            .fetch_all(&public)
            .await
            .unwrap();
    assert_eq!(hashes, vec!["owned-synthetic-hash"; 2]);
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
    public.close().await;
    owner.close().await;
}
