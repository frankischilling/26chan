#![cfg(feature = "database-tests")]

use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError, create_post_with_metadata,
    find_post,
};
use chrono::Utc;
use sqlx::PgPool;

fn post(comment: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Randomizer fixture".into(),
        comment: comment.into(),
        deletion_hash: "fixture-not-a-password".into(),
        sage: false,
    }
}

fn metadata(options: &str) -> PostMetadata<'_> {
    PostMetadata {
        keys: PostIdentityKeys {
            tripcode: None,
            poster_id: None,
        },
        country_database: None,
        flag: "",
        options,
    }
}

async fn create(pool: &PgPool, board: &str, parent: i64, options: &str) -> Result<i64, StoreError> {
    create_post_with_metadata(
        pool,
        board,
        parent,
        &post("ordinary comment"),
        None,
        PostingContext {
            request_start: Utc::now(),
            peer: None,
            op_password_proof: None,
        },
        metadata(options),
    )
    .await
}

#[tokio::test]
async fn board_randomizers_are_guarded_persisted_and_rollback_with_the_post() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text, '-', ''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,dice_roll,fortune_trip) VALUES ($1,'Randomizer test','Synthetic',2000,100,100,100,10,false,false)",
    )
    .bind(&board)
    .execute(&admin)
    .await
    .unwrap();

    let task_public = public.clone();
    let task_admin = admin.clone();
    let task_board = board.clone();
    let outcome = tokio::spawn(async move {
        // Disabled source features do not interpret dice-looking options.
        let plain = create(&task_public, &task_board, 0, "dice+0d0")
            .await
            .unwrap();
        let saved = find_post(&task_public, &task_board, plain).await.unwrap();
        assert_eq!(saved.dice_result, None);
        assert_eq!(saved.fortune_text, None);

        sqlx::query("UPDATE content.boards SET dice_roll=true WHERE slug=$1")
            .bind(&task_board)
            .execute(&task_admin)
            .await
            .unwrap();
        assert!(matches!(
            create(&task_public, &task_board, plain, "dice+0d6").await,
            Err(StoreError::Invalid("Dice roll count must be at least one."))
        ));

        // One-sided dice make the exact retained string deterministic while
        // still exercising server-side generation and the source formatting.
        let reply = create(&task_public, &task_board, plain, "prefix dice+2d1+3 suffix")
            .await
            .unwrap();
        let first = find_post(&task_public, &task_board, reply).await.unwrap();
        let second = find_post(&task_public, &task_board, reply).await.unwrap();
        assert_eq!(
            first.dice_result.as_deref(),
            Some("Rolled 1, 1 + 3 = 5 (2d1 + 3)")
        );
        assert_eq!(second.dice_result, first.dice_result);

        sqlx::query("UPDATE content.boards SET fortune_trip=true WHERE slug=$1")
            .bind(&task_board)
            .execute(&task_admin)
            .await
            .unwrap();
        // Generation happens after the board lock but before thread mutation.
        // A rejected parent rolls back every generated post-side effect.
        assert!(matches!(
            create(&task_public, &task_board, i64::MAX - 1, "fortune").await,
            Err(StoreError::NotFound)
        ));
        let leaked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM content.posts WHERE board=$1 AND fortune_text IS NOT NULL",
        )
        .bind(&task_board)
        .fetch_one(&task_admin)
        .await
        .unwrap();
        assert_eq!(leaked, 0);

        let fortune = create(&task_public, &task_board, plain, "fortune")
            .await
            .unwrap();
        let saved = find_post(&task_public, &task_board, fortune).await.unwrap();
        let text = saved.fortune_text.clone().expect("standard fortune");
        let color = saved.fortune_color.clone().expect("fortune color");
        assert!(!text.is_empty());
        assert_eq!(color.len(), 7);
        assert!(color.starts_with('#'));
        assert!(color[1..].bytes().all(|byte| byte.is_ascii_hexdigit()));
        let refreshed = find_post(&task_public, &task_board, fortune).await.unwrap();
        assert_eq!(refreshed.fortune_text.as_deref(), Some(text.as_str()));
        assert_eq!(refreshed.fortune_color.as_deref(), Some(color.as_str()));
    })
    .await;

    // Cleanup runs even when an assertion in the spawned exercise panics.
    sqlx::query(
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
    )
    .bind(&board)
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    outcome.unwrap()
}
