#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::Capability;
use board_store::{NewPost, StoreError, anonymous_session::PostingSession};
use sqlx::PgPool;

// An ordinary reader distinct from the OP fixture actor. Keep this identity
// stable across requests so runtime OP ownership and self-bump rules stay real.
fn reader_context() -> board_store::PostingContext {
    board_store::PostingContext {
        request_start: chrono::Utc::now(),
        peer: Some("192.0.2.202".parse().unwrap()),
        op_password_proof: None,
    }
}

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
    // Protected OPs can exceed rollover capacity; give this owned lifecycle
    // fixture room to exercise rollover independently of the actor OP quota.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES ($1,'Archive fixture','Synthetic owned data',100,20,10,1,1,0,0,0,100)").bind(&slug).execute(&owner).await.unwrap();
    assert!(
        board_store::board(&public, &slug)
            .await
            .unwrap()
            .expire_neglected,
        "New boards inherit source default"
    );
    let reporter = Capability::generate().unwrap();
    let report_token = reporter.storage_hash();
    let report_session = PostingSession {
        fingerprints: reporter.fingerprints(reader_context().peer, *b"US"),
        minted: true,
        now: chrono::Utc::now(),
    };
    let test_slug = slug.clone();
    let test_public = public.clone();
    let test_owner = owner.clone();
    let result = tokio::spawn(async move {
        let first = support::create_post(&test_public, &test_slug, 0, &post())
            .await
            .unwrap();
        let second = support::create_post(&test_public, &test_slug, 0, &post())
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
        archive_lifecycle(
            &test_owner,
            &test_public,
            &test_slug,
            second,
            report_session,
        )
        .await;
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
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(report_token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    result.unwrap();
}

async fn archive_lifecycle(
    owner: &PgPool,
    public: &PgPool,
    slug: &str,
    second: i64,
    report_session: PostingSession,
) {
    let report_identity =
        support::key(slug).public_report_rate_identity(reader_context().peer.unwrap());
    sqlx::query(
        "UPDATE content.boards SET archive_retention_seconds=3600,archive_limit=2 WHERE slug=$1",
    )
    .bind(slug)
    .execute(owner)
    .await
    .unwrap();
    let third = support::create_post(public, slug, 0, &post())
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
        support::create_post(public, slug, second, &post()).await,
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
    let fourth = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let fifth = support::create_post(public, slug, 0, &post())
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
        board_store::report_with_anonymous_session(
            public,
            slug,
            third,
            "Synthetic report",
            &report_identity,
            report_session
        )
        .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        board_store::delete_post(public, slug, third).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        support::create_post(public, slug, third, &post()).await,
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
    board_store::report_with_anonymous_session(
        public,
        slug,
        fourth,
        "Visible archive report",
        &report_identity,
        report_session,
    )
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
    let alongside_pinned = support::create_post(public, slug, 0, &post())
        .await
        .expect("Pinned threads do not consume ordinary capacity");
    assert_eq!(
        board_store::threads(public, slug, 0, 10)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(
        board_store::thread(public, slug, fifth)
            .await
            .unwrap()
            .archived_at
            .is_none()
    );
    // Keep the subsequent sage/bump scenario independent of this new OP.
    board_store::delete_post(public, slug, alongside_pinned)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET sticky=false,bumped_at=clock_timestamp()-interval '2 hours' WHERE id=$1").bind(fifth).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.boards SET thread_limit=2,archive_limit=1000 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let sixth = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let mut sage = post();
    sage.sage = true;
    support::create_post_with_context(public, slug, fifth, &sage, None, reader_context())
        .await
        .unwrap();
    let seventh = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(
        board_store::thread(public, slug, fifth)
            .await
            .unwrap()
            .archived_at
            .is_some()
    );
    // Make the victim ordering explicit even when every request lands in the
    // same second. Only the real public reply below may move sixth ahead.
    sqlx::query("UPDATE content.threads SET bumped_at=CASE WHEN id=$2 THEN '2000-01-01'::timestamptz ELSE '2001-01-01'::timestamptz END WHERE board=$1 AND id=ANY($3)")
        .bind(slug)
        .bind(sixth)
        .bind(vec![sixth, seventh])
        .execute(owner)
        .await
        .unwrap();
    let sixth_before = board_store::thread(public, slug, sixth).await.unwrap();
    let seventh_before = board_store::thread(public, slug, seventh).await.unwrap();
    assert!(sixth_before.bumped_at < seventh_before.bumped_at);
    // The legacy no-peer call was an ordinary non-OP reply. Give this reader
    // its own stable trusted peer so the OP self-bump timer stays irrelevant.
    support::create_post_with_context(public, slug, sixth, &post(), None, reader_context())
        .await
        .unwrap();
    assert!(
        board_store::thread(public, slug, sixth)
            .await
            .unwrap()
            .bumped_at
            > seventh_before.bumped_at,
        "The actual public reply must bump sixth ahead of seventh"
    );
    let eighth = support::create_post(public, slug, 0, &post())
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
    // Use distinct actors: request timestamps can reach the actor gate out of
    // order, so sharing one peer can correctly trip its cooldown even at zero
    // delay. This scenario tests board rollover, not same-actor admission.
    let mut tasks = tokio::task::JoinSet::new();
    for actor in 1..=12 {
        let pool = public.clone();
        let slug = slug.to_owned();
        tasks.spawn(async move {
            support::create_post_with_context(
                &pool,
                &slug,
                0,
                &post(),
                None,
                board_store::PostingContext {
                    request_start: chrono::Utc::now(),
                    peer: Some(std::net::Ipv4Addr::new(198, 51, 100, actor).into()),
                    op_password_proof: None,
                },
            )
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
    source_rollover_order(owner, public, slug).await;
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
        let target = support::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        let other = support::create_post(public, slug, 0, &post())
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
            support::create_post_with_context(
                &first_pool,
                &first_slug,
                if reply_first { target } else { 0 },
                &post(),
                None,
                reader_context(),
            )
            .await
        });
        wait_behind(owner, one_pid, owner_pid).await;
        let second_pool = two.clone();
        let second_slug = slug.to_owned();
        let second = tokio::spawn(async move {
            support::create_post_with_context(
                &second_pool,
                &second_slug,
                if reply_first { 0 } else { target },
                &post(),
                None,
                reader_context(),
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
    queued_op_observes_protection(owner, public, slug, &one, one_pid).await;
    one.close().await;
    two.close().await;
    sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    let pinned = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE content.threads SET sticky=true,bumped_at='2000-01-01'::timestamptz WHERE id=$1",
    )
    .bind(pinned)
    .execute(owner)
    .await
    .unwrap();
    let undead = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE content.threads SET undead=true,bumped_at='2000-01-02'::timestamptz WHERE id=$1",
    )
    .bind(undead)
    .execute(owner)
    .await
    .unwrap();
    let both = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET sticky=true,undead=true,bumped_at='2000-01-03'::timestamptz WHERE id=$1")
        .bind(both).execute(owner).await.unwrap();
    let ordinary = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET bumped_at='2001-01-01'::timestamptz WHERE id=$1")
        .bind(ordinary)
        .execute(owner)
        .await
        .unwrap();
    let other = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    for id in [pinned, undead, both, ordinary, other] {
        assert!(
            board_store::thread(public, slug, id)
                .await
                .unwrap()
                .archived_at
                .is_none(),
            "Three protected OPs must leave both ordinary slots available"
        );
    }
    let newest = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    for id in [pinned, undead, both, other, newest] {
        assert!(
            board_store::thread(public, slug, id)
                .await
                .unwrap()
                .archived_at
                .is_none()
        );
    }
    assert_complete_protected_listing(public, slug, [pinned, undead, both, other, newest]).await;
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
    // The same protection predicate applies when ordinary rollover soft-deletes.
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
        .bind(slug)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET bumped_at='2001-01-01'::timestamptz WHERE id=$1")
        .bind(other)
        .execute(owner)
        .await
        .unwrap();
    let replacement = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    assert!(matches!(
        board_store::thread(public, slug, other).await,
        Err(StoreError::NotFound)
    ));
    let deleted: bool = sqlx::query_scalar("SELECT deleted FROM content.threads WHERE id=$1")
        .bind(other)
        .fetch_one(owner)
        .await
        .unwrap();
    assert!(
        deleted,
        "Rollover must soft-delete, not physically erase the ordinary OP"
    );
    for id in [pinned, undead, both, newest, replacement] {
        assert!(
            board_store::thread(public, slug, id)
                .await
                .unwrap()
                .archived_at
                .is_none()
        );
    }
    // Owned fixtures exceed the default ceiling without relaxing schema bounds.
    sqlx::query("WITH roots AS (INSERT INTO content.threads(board,sticky) SELECT $1,true FROM generate_series(1,$2::integer) RETURNING id,board) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,board,id,'Anonymous','Protected read bound','Owned fixture' FROM roots")
        .bind(slug).bind(board_store::MAX_BOARD_READ_THREADS as i32 - 4)
        .execute(owner).await.unwrap();
    assert!(matches!(
        board_store::board_snapshot(public, slug, board_store::BoardSelection::All, None).await,
        Err(StoreError::ReadLimit)
    ));
    assert!(matches!(
        board_store::json_board_snapshot(public, slug, board_store::BoardSelection::All, 0).await,
        Err(StoreError::ReadLimit)
    ));
    assert!(matches!(
        board_store::board_page_snapshot(public, slug, board_store::BoardSelection::All, Some(0))
            .await,
        Err(StoreError::ReadLimit)
    ));
}

async fn queued_op_observes_protection(
    owner: &PgPool,
    public: &PgPool,
    slug: &str,
    queued_pool: &PgPool,
    queued_pid: i32,
) {
    for sticky in [true, false] {
        sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
            .bind(slug)
            .execute(owner)
            .await
            .unwrap();
        let protected = support::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        let ordinary = support::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET bumped_at=CASE WHEN id=$2 THEN '2000-01-01'::timestamptz ELSE '2000-01-02'::timestamptz END WHERE board=$1 AND NOT deleted")
            .bind(slug).bind(protected).execute(owner).await.unwrap();
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
        let pool = queued_pool.clone();
        let board = slug.to_owned();
        let queued =
            tokio::spawn(async move { support::create_post(&pool, &board, 0, &post()).await });
        // Observe the actual PostgreSQL lock dependency before changing protection.
        // Elapsed time alone cannot establish that the OP is queued.
        wait_behind(owner, queued_pid, owner_pid).await;
        sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3 WHERE id=$1")
            .bind(protected)
            .bind(sticky)
            .bind(!sticky)
            .execute(&mut *lock)
            .await
            .unwrap();
        lock.commit().await.unwrap();
        let created = queued.await.unwrap().unwrap();
        let protected_thread = board_store::thread(public, slug, protected).await.unwrap();
        assert_eq!(protected_thread.sticky, sticky);
        assert_eq!(protected_thread.undead, !sticky);
        for id in [protected, ordinary, created] {
            assert!(
                board_store::thread(public, slug, id)
                    .await
                    .unwrap()
                    .archived_at
                    .is_none(),
                "Queued OP must use protection committed before its board lock is granted"
            );
        }
        assert_eq!(
            board_store::threads(public, slug, 0, 10)
                .await
                .unwrap()
                .len(),
            3
        );
        support::create_post(public, slug, 0, &post())
            .await
            .unwrap();
        assert!(
            board_store::thread(public, slug, ordinary)
                .await
                .unwrap()
                .archived_at
                .is_some()
        );
        assert!(
            board_store::thread(public, slug, protected)
                .await
                .unwrap()
                .archived_at
                .is_none()
        );
    }
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
    let older = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let expired = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    let newest = support::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp()-interval '3 hours',archive_expires_at=statement_timestamp()+interval '1 hour' WHERE id=$1").bind(older).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp()-interval '2 hours',archive_expires_at=statement_timestamp()-interval '1 hour' WHERE id=$1").bind(expired).execute(owner).await.unwrap();
    support::create_post(public, slug, 0, &post())
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

async fn assert_complete_protected_listing(public: &PgPool, slug: &str, expected: [i64; 5]) {
    // All projections must expose protected extras above ordinary capacity (2).
    let html =
        board_store::board_page_snapshot(public, slug, board_store::BoardSelection::All, Some(0))
            .await
            .unwrap()
            .snapshot;
    let catalog =
        board_store::json_board_snapshot(public, slug, board_store::BoardSelection::All, 0)
            .await
            .unwrap();
    let thread_list =
        board_store::board_snapshot(public, slug, board_store::BoardSelection::All, None)
            .await
            .unwrap();
    let exact = board_store::board_snapshot_all_bounded(public, slug, Some(0), expected.len())
        .await
        .unwrap();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    for snapshot in [html, catalog, thread_list, exact] {
        assert_eq!(snapshot.board.thread_limit, 2);
        assert!(!snapshot.has_next);
        let mut ids = snapshot
            .threads
            .iter()
            .map(|entry| entry.thread.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(
            ids, expected,
            "Complete projections must not truncate to ordinary capacity"
        );
    }
    for replies in [None, Some(0), Some(5)] {
        assert!(
            matches!(
                board_store::board_snapshot_all_bounded(public, slug, replies, 4).await,
                Err(StoreError::ReadLimit)
            ),
            "A lower complete-read limit must fail, never return a truncated listing"
        );
    }
    for invalid in [0, board_store::MAX_BOARD_READ_THREADS + 1] {
        assert!(matches!(
            board_store::board_snapshot_all_bounded(public, slug, None, invalid).await,
            Err(StoreError::Invalid(_))
        ));
    }
    // Numbered-page policy remains separately bounded by ordinary capacity.
    let page =
        board_store::board_snapshot(public, slug, board_store::BoardSelection::Page(2), None)
            .await
            .unwrap();
    assert_eq!(page.threads.len(), 1);
    assert!(!page.has_next);
    assert!(matches!(
        board_store::board_snapshot(public, slug, board_store::BoardSelection::Page(3), None).await,
        Err(StoreError::PageNotFound)
    ));
}

async fn source_rollover_order(owner: &PgPool, public: &PgPool, slug: &str) {
    // Imported /f/ is the only active source override; /test/'s no is commented.
    for source in [
        "f", "b", "r9k", "trash", "soc", "y", "bant", "u", "test", "j",
    ] {
        assert_eq!(
            board_store::board(owner, source)
                .await
                .unwrap()
                .expire_neglected,
            source != "f",
            "/{source}/ source rollover policy"
        );
    }
    let denied = sqlx::query("UPDATE content.boards SET expire_neglected=false WHERE slug=$1")
        .bind(slug)
        .execute(public)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let queued_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queued_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&queued_pool)
        .await
        .unwrap();
    for retention in [0, 3600] {
        for expire_neglected in [false, true] {
            for equal_clocks in [false, true] {
                sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1")
                    .bind(slug)
                    .execute(owner)
                    .await
                    .unwrap();
                sqlx::query("UPDATE content.boards SET thread_limit=2,archive_limit=1000,archive_retention_seconds=$2,expire_neglected=$3 WHERE slug=$1")
                    .bind(slug).bind(retention).bind(!expire_neglected).execute(owner).await.unwrap();
                let mut protected = Vec::new();
                for (sticky, undead) in [(true, false), (false, true), (true, true)] {
                    let id = support::create_post(public, slug, 0, &post())
                        .await
                        .unwrap();
                    sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3,bumped_at='1999-01-01Z' WHERE id=$1")
                        .bind(id).bind(sticky).bind(undead).execute(owner).await.unwrap();
                    protected.push(id);
                }
                let first = support::create_post(public, slug, 0, &post())
                    .await
                    .unwrap();
                let second = support::create_post(public, slug, 0, &post())
                    .await
                    .unwrap();
                assert!(first < second);
                // Lower OP ID has the NEWER bump clock: fixed bump ordering or
                // fixed ID ordering necessarily fails one of the two policies.
                sqlx::query("UPDATE content.threads SET bumped_at=CASE WHEN id=$2 AND NOT $4 THEN '2001-01-02Z'::timestamptz ELSE '2001-01-01Z'::timestamptz END WHERE board=$1 AND id=ANY($3)")
                    .bind(slug).bind(first).bind(vec![first, second]).bind(equal_clocks)
                    .execute(owner).await.unwrap();
                let original_ids = [protected.clone(), vec![first, second]].concat();
                let clocks: Vec<(i64, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
                    sqlx::query_as("SELECT id,created_at,bumped_at FROM content.threads WHERE id=ANY($1) ORDER BY id")
                        .bind(&original_ids).fetch_all(owner).await.unwrap();
                let posts: Vec<serde_json::Value> = sqlx::query_scalar(
                    "SELECT to_jsonb(p) FROM content.posts p WHERE id=ANY($1) ORDER BY id",
                )
                .bind(&original_ids)
                .fetch_all(owner)
                .await
                .unwrap();
                // Observe an actual queued writer, then change policy while
                // holding its board lock. It must use the newly committed value.
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
                let pool = queued_pool.clone();
                let board = slug.to_owned();
                let queued =
                    tokio::spawn(
                        async move { support::create_post(&pool, &board, 0, &post()).await },
                    );
                wait_behind(owner, queued_pid, owner_pid).await;
                sqlx::query("UPDATE content.boards SET expire_neglected=$2 WHERE slug=$1")
                    .bind(slug)
                    .bind(expire_neglected)
                    .execute(&mut *lock)
                    .await
                    .unwrap();
                lock.commit().await.unwrap();
                let created = queued.await.unwrap().unwrap();
                assert_eq!(
                    board_store::board(public, slug)
                        .await
                        .unwrap()
                        .expire_neglected,
                    expire_neglected
                );
                let victim = if expire_neglected && !equal_clocks {
                    second
                } else {
                    first
                };
                let survivor = if victim == first { second } else { first };
                let states: Vec<(i64, bool, bool)> = sqlx::query_as("SELECT id,deleted,archived_at IS NOT NULL FROM content.threads WHERE id=ANY($1) ORDER BY id")
                    .bind(&original_ids).fetch_all(owner).await.unwrap();
                assert_eq!(states.len(), 5);
                for (id, deleted, archived) in states {
                    assert_eq!(
                        (deleted, archived),
                        (
                            id == victim && retention == 0,
                            id == victim && retention > 0
                        ),
                        "retention={retention}, expire_neglected={expire_neglected}, equal_clocks={equal_clocks}, id={id}"
                    );
                }
                for id in [protected.clone(), vec![survivor, created]].concat() {
                    assert!(
                        board_store::thread(public, slug, id)
                            .await
                            .unwrap()
                            .archived_at
                            .is_none()
                    );
                }
                let after_clocks: Vec<(i64, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
                    sqlx::query_as("SELECT id,created_at,bumped_at FROM content.threads WHERE id=ANY($1) ORDER BY id")
                        .bind(&original_ids).fetch_all(owner).await.unwrap();
                assert_eq!(
                    after_clocks, clocks,
                    "Rollover preserves OP creation/bump clocks"
                );
                let after_posts: Vec<serde_json::Value> = sqlx::query_scalar(
                    "SELECT to_jsonb(p) FROM content.posts p WHERE id=ANY($1) ORDER BY id",
                )
                .bind(&original_ids)
                .fetch_all(owner)
                .await
                .unwrap();
                assert_eq!(
                    after_posts, posts,
                    "Rollover preserves every original post and timestamp"
                );
            }
        }
    }
    queued_pool.close().await;
}
