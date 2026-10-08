#![cfg(feature = "database-tests")]
mod support;
use board_store::{NewPost, StoreError};

#[tokio::test]
async fn thread_metadata_and_posts_stay_consistent_during_writes() {
    let url =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&url).await.unwrap();
    let post = NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "A snapshot consistency fixture".into(),
        deletion_hash: "fixture-not-a-valid-password-hash".into(),
        sage: false,
    };
    let id = support::create_post(&pool, "fixture", 0, &post)
        .await
        .unwrap();
    let writer_pool = pool.clone();
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let writer_key = support::key("fixture");
    let writer_actor = writer_key.public_posting_rate_identity(support::peer());
    let writer = tokio::spawn(async move {
        for _ in 0..80 {
            // Independent snapshot writes: keep imported policy unchanged and
            // clear only this owned actor’s earlier fixture reply history.
            sqlx::query(
                "DELETE FROM post_secrets.posting_history WHERE actor_hash=$1 AND board='fixture'",
            )
            .bind(writer_actor.as_bytes().as_slice())
            .execute(&owner)
            .await
            .unwrap();
            support::create_post(&writer_pool, "fixture", id, &post)
                .await
                .unwrap();
        }
    });
    for _ in 0..160 {
        let board_store::ThreadSnapshot {
            board,
            thread: metadata,
            posts,
            ..
        } = board_store::thread_snapshot(&pool, "fixture", id)
            .await
            .unwrap();
        assert_eq!(board.slug, "fixture");
        assert_eq!(metadata.reply_count as usize, posts.len() - 1);
    }
    writer.await.unwrap();
    board_store::delete_post(&pool, "fixture", id)
        .await
        .unwrap();
    assert!(matches!(
        board_store::thread_snapshot(&pool, "fixture", id).await,
        Err(StoreError::NotFound)
    ));
    pool.close().await;
}

#[tokio::test]
async fn concurrent_replies_enforce_lifetime_limit_and_sage_does_not_bump() {
    let url =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&url).await.unwrap();
    let post = NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "A concurrency fixture".into(),
        deletion_hash: "fixture-not-a-valid-password-hash".into(),
        sage: false,
    };
    let thread = support::create_post(&pool, "limit", 0, &post)
        .await
        .unwrap();
    let before = board_store::thread(&pool, "limit", thread).await.unwrap();
    let mut sage = post.clone();
    sage.sage = true;
    // A distinct reader ensures sage itself suppresses this bump, rather than
    // accidentally passing because the source OP self-bump timer also applies.
    support::create_post_with_context(
        &pool,
        "limit",
        thread,
        &sage,
        None,
        board_store::PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some("192.0.2.202".parse().unwrap()),
            op_password_proof: None,
        },
    )
    .await
    .unwrap();
    let after = board_store::thread(&pool, "limit", thread).await.unwrap();
    assert_eq!(before.bumped_at, after.bumped_at);
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let pool = pool.clone();
        let post = post.clone();
        // Each contending writer is an independent trusted actor. The test
        // asserts the shared thread limit, not per-actor posting cooldowns.
        let key = support::fresh_key();
        jobs.spawn(async move {
            board_store::create_post_with_identity_keys(
                &pool,
                "limit",
                thread,
                &post,
                None,
                board_store::PostingContext {
                    request_start: chrono::Utc::now(),
                    peer: Some(support::peer()),
                    op_password_proof: None,
                },
                board_store::PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&key),
                },
            )
            .await
        });
    }
    let mut accepted = 0;
    while let Some(result) = jobs.join_next().await {
        match result.unwrap() {
            Ok(_) => accepted += 1,
            Err(StoreError::Conflict(_)) => {}
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(accepted, 2);
    assert_eq!(
        board_store::posts(&pool, "limit", thread)
            .await
            .unwrap()
            .len(),
        4
    );
    let preview = board_store::preview_posts(&pool, "limit", thread, 1)
        .await
        .unwrap();
    assert_eq!(preview.len(), 2);
    assert_eq!(preview[0].id, thread);
    assert_eq!(
        preview[1].id,
        board_store::posts(&pool, "limit", thread)
            .await
            .unwrap()
            .last()
            .unwrap()
            .id
    );
    assert_eq!(
        board_store::visible_post_count(&pool, "limit", thread)
            .await
            .unwrap(),
        4
    );
    pool.close().await;
}
