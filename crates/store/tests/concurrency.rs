#![cfg(feature = "database-tests")]
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
    let id = board_store::create_post(&pool, "test", 0, &post)
        .await
        .unwrap();
    let writer_pool = pool.clone();
    let writer = tokio::spawn(async move {
        for _ in 0..80 {
            board_store::create_post(&writer_pool, "test", id, &post)
                .await
                .unwrap();
        }
    });
    for _ in 0..160 {
        let board_store::ThreadSnapshot {
            board,
            thread: metadata,
            posts,
        } = board_store::thread_snapshot(&pool, "test", id)
            .await
            .unwrap();
        assert_eq!(board.slug, "test");
        assert_eq!(metadata.reply_count as usize, posts.len() - 1);
    }
    writer.await.unwrap();
    board_store::delete_post(&pool, "test", id).await.unwrap();
    assert!(matches!(
        board_store::thread_snapshot(&pool, "test", id).await,
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
    let thread = board_store::create_post(&pool, "limit", 0, &post)
        .await
        .unwrap();
    let before = board_store::thread(&pool, "limit", thread).await.unwrap();
    let mut sage = post.clone();
    sage.sage = true;
    board_store::create_post(&pool, "limit", thread, &sage)
        .await
        .unwrap();
    let after = board_store::thread(&pool, "limit", thread).await.unwrap();
    assert_eq!(before.bumped_at, after.bumped_at);
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let pool = pool.clone();
        let post = post.clone();
        jobs.spawn(async move { board_store::create_post(&pool, "limit", thread, &post).await });
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
