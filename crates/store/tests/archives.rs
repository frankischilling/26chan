#![cfg(feature = "database-tests")]

use board_store::{NewPost, StoreError};
use sqlx::PgPool;

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Synthetic archive".into(),
        comment: "Owned archive lifecycle fixture".into(),
        deletion_hash: "not-a-real-password-hash".into(),
        sage: false,
    }
}

#[tokio::test]
async fn bounded_boards_roll_over_through_the_actual_public_role() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let slug = format!("a{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Archive fixture','Synthetic owned data',100,20,10,1,1)").bind(&slug).execute(&owner).await.unwrap();
    let test_slug = slug.clone();
    let test_public = public.clone();
    let test_owner = owner.clone();
    let result = tokio::spawn(async move {
        let first = board_store::create_post(&test_public, &test_slug, 0, &post())
            .await
            .unwrap();
        let second = board_store::create_post(&test_public, &test_slug, 0, &post())
            .await
            .expect("A new thread displaces the oldest unpinned thread");
        assert!(matches!(
            board_store::thread(&test_public, &test_slug, first).await,
            Err(StoreError::NotFound)
        ));
        assert_eq!(
            board_store::threads(&test_public, &test_slug, 0, 10)
                .await
                .unwrap()[0]
                .id,
            second
        );
        archive_lifecycle(&test_owner, &test_public, &test_slug, second).await;
    })
    .await;
    sqlx::query("DELETE FROM content.reports WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
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
    public.close().await;
    owner.close().await;
    result.unwrap();
}

async fn archive_lifecycle(owner: &PgPool, public: &PgPool, slug: &str, second: i64) {
    sqlx::query(
        "UPDATE content.boards SET archive_retention_seconds=3600,archive_limit=2 WHERE slug=$1",
    )
    .bind(slug)
    .execute(owner)
    .await
    .unwrap();
    let third = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let archived = board_store::archive_snapshot(public, slug).await.unwrap();
    assert_eq!(
        archived
            .entries
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![second]
    );
    let metadata = board_store::thread(public, slug, second).await.unwrap();
    assert!(metadata.archived_at.is_some());
    assert!(
        !metadata.closed,
        "Archiving must not require public closed/sticky authority"
    );
    assert!(matches!(
        board_store::create_post(public, slug, second, &post()).await,
        Err(StoreError::Conflict(_))
    ));
    for statement in [
        "UPDATE content.threads SET closed=false WHERE false",
        "UPDATE content.threads SET sticky=false WHERE false",
        "UPDATE content.boards SET archive_retention_seconds=0 WHERE false",
        "SELECT id FROM staff_identity.accounts LIMIT 1",
    ] {
        let error = sqlx::query(statement).execute(public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        assert_eq!(
            sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(public)
                .await
                .unwrap(),
            1
        );
    }
    let fourth = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let fifth = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(matches!(
        board_store::thread(public, slug, second).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        board_store::archive_snapshot(public, slug)
            .await
            .unwrap()
            .entries
            .len(),
        2
    );
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1").bind(third).execute(owner).await.unwrap();
    assert!(matches!(
        board_store::thread_snapshot(public, slug, third).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        board_store::find_post(public, slug, third).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        board_store::posts(public, slug, third)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        board_store::preview_posts(public, slug, third, 5)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        board_store::visible_post_count(public, slug, third)
            .await
            .unwrap(),
        0
    );
    assert!(matches!(
        board_store::report(public, slug, third, "Synthetic report").await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        board_store::delete_post(public, slug, third).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        board_store::create_post(public, slug, third, &post()).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        board_store::archive_snapshot(public, slug)
            .await
            .unwrap()
            .entries[0]
            .id,
        fourth
    );
    board_store::report(public, slug, fourth, "Visible archive report")
        .await
        .unwrap();
    board_store::delete_post(public, slug, fourth)
        .await
        .unwrap();
    assert!(
        board_store::archive_snapshot(public, slug)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
        .bind(fifth)
        .execute(owner)
        .await
        .unwrap();
    assert!(matches!(
        board_store::create_post(public, slug, 0, &post()).await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(
        board_store::threads(public, slug, 0, 10).await.unwrap()[0].id,
        fifth
    );
    sqlx::query("UPDATE content.threads SET sticky=false,bumped_at=clock_timestamp()-interval '2 hours' WHERE id=$1").bind(fifth).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.boards SET thread_limit=2,archive_limit=1000 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let sixth = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let mut sage = post();
    sage.sage = true;
    board_store::create_post(public, slug, fifth, &sage)
        .await
        .unwrap();
    let seventh = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(
        board_store::thread(public, slug, fifth)
            .await
            .unwrap()
            .archived_at
            .is_some()
    );
    board_store::create_post(public, slug, sixth, &post())
        .await
        .unwrap();
    let eighth = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(
        board_store::thread(public, slug, seventh)
            .await
            .unwrap()
            .archived_at
            .is_some()
    );
    assert!(
        board_store::thread(public, slug, sixth)
            .await
            .unwrap()
            .archived_at
            .is_none()
    );
    // Concurrent new OPs serialize without exceeding active capacity or losing posts.
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let pool = public.clone();
        let slug = slug.to_owned();
        tasks.spawn(async move {
            board_store::create_post(&pool, &slug, 0, &post())
                .await
                .unwrap()
        });
    }
    let mut created = Vec::new();
    while let Some(result) = tasks.join_next().await {
        created.push(result.unwrap());
    }
    assert_eq!(
        board_store::threads(public, slug, 0, 1000)
            .await
            .unwrap()
            .len(),
        2
    );
    for id in created {
        assert_eq!(board_store::posts(public, slug, id).await.unwrap().len(), 1);
    }
    assert!(
        board_store::thread(public, slug, eighth)
            .await
            .unwrap()
            .archived_at
            .is_some()
    );
    // Disabled policy hides old archive entries immediately, without a cleanup job.
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    assert!(matches!(
        board_store::archive_snapshot(public, slug).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        board_store::thread(public, slug, eighth).await,
        Err(StoreError::NotFound)
    ));
    coherent_archive_snapshot(owner, public, slug).await;
    expired_entries_do_not_displace_valid_archives(owner, public, slug).await;
    reply_and_rollover_serialize(owner, public, slug).await;
}

async fn wait_behind(owner: &PgPool, pid: i32, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual public mutation must wait behind the expected lock owner");
}

async fn reply_and_rollover_serialize(owner: &PgPool, public: &PgPool, slug: &str) {
    let url = std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap();
    let one = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let two = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let one_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&one)
        .await
        .unwrap();
    let two_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&two)
        .await
        .unwrap();
    for reply_first in [true, false] {
        sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
            .bind(slug)
            .execute(owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET thread_limit=2,archive_limit=1000 WHERE slug=$1")
            .bind(slug)
            .execute(owner)
            .await
            .unwrap();
        let target = board_store::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        let other = board_store::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET bumped_at=CASE WHEN id=$2 THEN '2000-01-01'::timestamptz ELSE '2000-01-02'::timestamptz END WHERE board=$1 AND NOT deleted").bind(slug).bind(target).execute(owner).await.unwrap();
        let mut lock = owner.begin().await.unwrap();
        let owner_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(slug)
            .execute(&mut *lock)
            .await
            .unwrap();
        let first_pool = one.clone();
        let first_slug = slug.to_owned();
        let first = tokio::spawn(async move {
            board_store::create_post(
                &first_pool,
                &first_slug,
                if reply_first { target } else { 0 },
                &post(),
            )
            .await
        });
        wait_behind(owner, one_pid, owner_pid).await;
        let second_pool = two.clone();
        let second_slug = slug.to_owned();
        let second = tokio::spawn(async move {
            board_store::create_post(
                &second_pool,
                &second_slug,
                if reply_first { 0 } else { target },
                &post(),
            )
            .await
        });
        wait_behind(owner, two_pid, one_pid).await;
        lock.commit().await.unwrap();
        first.await.unwrap().unwrap();
        let second = second.await.unwrap();
        let metadata = board_store::thread(public, slug, target).await.unwrap();
        let replies = board_store::posts(public, slug, target).await.unwrap();
        if reply_first {
            second.unwrap();
            assert_eq!(metadata.reply_count, 1);
            assert_eq!(replies.len(), 2);
            assert!(
                metadata.archived_at.is_none(),
                "successful reply bump protects the target from rollover"
            );
            assert!(
                board_store::thread(public, slug, other)
                    .await
                    .unwrap()
                    .archived_at
                    .is_some()
            );
        } else {
            assert!(matches!(second, Err(StoreError::Conflict(_))));
            assert_eq!(metadata.reply_count, 0);
            assert_eq!(replies.len(), 1);
            assert!(
                metadata.archived_at.is_some(),
                "rollover wins before reply authorization"
            );
        }
        assert_eq!(
            board_store::threads(public, slug, 0, 1000)
                .await
                .unwrap()
                .len(),
            2
        );
    }
    one.close().await;
    two.close().await;
    sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let pinned = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let ordinary = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE content.threads SET sticky=true,bumped_at='2000-01-01'::timestamptz WHERE id=$1",
    )
    .bind(pinned)
    .execute(owner)
    .await
    .unwrap();
    board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(
        board_store::thread(public, slug, pinned)
            .await
            .unwrap()
            .archived_at
            .is_none()
    );
    let before = board_store::thread(public, slug, ordinary).await.unwrap();
    assert!(before.archived_at.is_some());
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=86400 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(
        board_store::thread(public, slug, ordinary)
            .await
            .unwrap()
            .archive_expires_at,
        before.archive_expires_at,
        "changing policy must not extend an existing archive lifetime"
    );
}

async fn expired_entries_do_not_displace_valid_archives(
    owner: &PgPool,
    public: &PgPool,
    slug: &str,
) {
    sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600,archive_limit=2,thread_limit=1 WHERE slug=$1").bind(slug).execute(owner).await.unwrap();
    let older = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let expired = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let newest = board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp()-interval '3 hours',archive_expires_at=statement_timestamp()+interval '1 hour' WHERE id=$1").bind(older).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp()-interval '2 hours',archive_expires_at=statement_timestamp()-interval '1 hour' WHERE id=$1").bind(expired).execute(owner).await.unwrap();
    board_store::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let ids = board_store::archive_snapshot(public, slug)
        .await
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![older, newest],
        "expired entries must not occupy archive capacity"
    );
}

async fn coherent_archive_snapshot(owner: &PgPool, public: &PgPool, slug: &str) {
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let expected = board_store::archive_snapshot(public, slug)
        .await
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert!(!expected.is_empty());
    let reader = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&reader)
        .await
        .unwrap();
    let mut owner_tx = owner.begin().await.unwrap();
    sqlx::query("LOCK TABLE content.posts IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    let read_pool = reader.clone();
    let read_slug = slug.to_owned();
    let task =
        tokio::spawn(async move { board_store::archive_snapshot(&read_pool, &read_slug).await });
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='content.posts'::regclass AND NOT granted)").bind(pid).fetch_one(owner).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("archive query must reach its second statement");
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
        .bind(slug)
        .execute(&mut *owner_tx)
        .await
        .unwrap();
    owner_tx.commit().await.unwrap();
    let snapshot = task.await.unwrap().unwrap();
    assert_eq!(snapshot.board.archive_retention_seconds, 3600);
    assert_eq!(
        snapshot
            .entries
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        expected,
        "one archive representation must retain its board/entries snapshot across the policy commit"
    );
    assert!(matches!(
        board_store::archive_snapshot(&reader, slug).await,
        Err(StoreError::NotFound)
    ));
    reader.close().await;
}
