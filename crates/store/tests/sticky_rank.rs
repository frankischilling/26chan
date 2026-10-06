#![cfg(feature = "database-tests")]

use board_store::{BoardSelection, StoreError};
use sqlx::PgPool;

async fn pool(variable: &str, expected: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, expected);
    pool
}

fn rejected(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>, code: &str) {
    let error = result.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some(code)
    );
}

async fn assert_order(public: &PgPool, board: &str, expected: &[i64]) {
    let list = board_store::threads(public, board, 0, 100).await.unwrap();
    assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), expected);
    let all = board_store::board_snapshot(public, board, BoardSelection::All, Some(0))
        .await
        .unwrap();
    assert_eq!(
        all.threads.iter().map(|t| t.thread.id).collect::<Vec<_>>(),
        expected
    );
    for (index, id) in expected.iter().enumerate() {
        let statistics = board_store::thread_statistics(public, board, *id)
            .await
            .unwrap();
        assert_eq!(statistics.page, Some(index as i64 / 2 + 1));
    }
    for (index, expected_page) in expected.chunks(2).enumerate() {
        let page = board_store::board_snapshot(
            public,
            board,
            BoardSelection::Page(index as i64 + 1),
            Some(0),
        )
        .await
        .unwrap();
        assert_eq!(
            page.threads.iter().map(|t| t.thread.id).collect::<Vec<_>>(),
            expected_page
        );
        assert_eq!(page.has_next, (index + 1) * 2 < expected.len());
    }
}

#[tokio::test]
async fn additive_rank_preserves_roles_visibility_boolean_protection_and_ordering() {
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let staff = pool("STAFF_DATABASE_URL", "board_staff").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("k{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,rss_enabled) VALUES($1,'Rank fixture','Owned synthetic data',2000,100,100,100,2,3600,true)")
        .bind(&board).execute(&owner).await.unwrap();
    let test_owner = owner.clone();
    let test_board = board.clone();
    let result = tokio::spawn(async move {
        let owner = test_owner;
        let board = test_board;
        let mut ids = Vec::new();
        for _ in 0..6 {
            // The legacy column list remains legal and defaults rank to zero.
            let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                .bind(&board).fetch_one(&public).await.unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Rank fixture','Owned synthetic fixture')")
                .bind(id).bind(&board).execute(&owner).await.unwrap();
            ids.push(id);
        }
        let ranks: Vec<i16> = sqlx::query_scalar("SELECT sticky_rank FROM content.threads WHERE board=$1")
            .bind(&board).fetch_all(&public).await.unwrap();
        assert_eq!(ranks, vec![0; 6]);
        sqlx::query("UPDATE content.threads SET bumped_at=to_timestamp(1000000000) WHERE board=$1")
            .bind(&board).execute(&owner).await.unwrap();
        assert_order(&public, &board, &ids.iter().copied().rev().collect::<Vec<_>>()).await;

        rejected(sqlx::query("UPDATE content.threads SET sticky_rank=1 WHERE id=$1")
            .bind(ids[0]).execute(&public).await, "42501");
        rejected(sqlx::query("INSERT INTO content.threads(board,sticky_rank) VALUES($1,1)")
            .bind(&board).execute(&public).await, "42501");
        rejected(sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
            .bind(ids[0]).execute(&public).await, "42501");
        for rank in [-1_i16, 61] {
            rejected(sqlx::query("UPDATE content.threads SET sticky_rank=$2 WHERE id=$1")
                .bind(ids[0]).bind(rank).execute(&staff).await, "23514");
        }
        rejected(sqlx::query("UPDATE content.threads SET sticky_rank=NULL WHERE id=$1")
            .bind(ids[0]).execute(&staff).await, "23502");
        // Higher ranks beat newer bumps; equal rank and clock fall back to ID.
        for (index, rank) in [(0, 60_i16), (1, 30), (2, 30), (3, 0)] {
            sqlx::query("UPDATE content.threads SET sticky=true,sticky_rank=$2 WHERE id=$1")
                .bind(ids[index]).bind(rank).execute(&staff).await.unwrap();
        }
        sqlx::query("UPDATE content.threads SET bumped_at=to_timestamp(1000000010) WHERE id=$1")
            .bind(ids[3]).execute(&owner).await.unwrap();
        // A stale rank on an ordinary thread has no display/protection effect.
        sqlx::query("UPDATE content.threads SET sticky_rank=60 WHERE id=$1")
            .bind(ids[4]).execute(&staff).await.unwrap();
        assert_order(&public, &board, &[ids[0], ids[2], ids[1], ids[3], ids[5], ids[4]]).await;
        let rss = board_store::rss_snapshot(&public, &board).await.unwrap();
        assert_eq!(rss.posts.iter().map(|p| p.id).collect::<Vec<_>>(), ids.iter().copied().rev().collect::<Vec<_>>());

        // Old sticky toggles still work without touching rank, which readers
        // ignore immediately after unstickying. Bump precedence stays intact.
        sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1")
            .bind(ids[0]).execute(&staff).await.unwrap();
        assert_order(&public, &board, &[ids[2], ids[1], ids[3], ids[5], ids[4], ids[0]]).await;
        rejected(sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(ids[3]).execute(&public).await, "23514");
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=ANY($1)")
            .bind(&[ids[0], ids[4]][..]).execute(&public).await.unwrap();
        let archive = board_store::archive_snapshot(&public, &board).await.unwrap();
        assert_eq!(archive.entries.iter().map(|t| t.id).collect::<Vec<_>>(), vec![ids[0], ids[4]]);
        assert_eq!(board_store::thread_statistics(&public, &board, ids[4]).await.unwrap().page, None);

        // New audit masks are restricted to changed grouped actions. Legacy
        // inserts retain their column list and every former action remains valid.
        for action in ["close", "reopen", "sticky", "unsticky", "permasage", "unpermasage", "permaage", "unpermaage", "remove-post", "remove-file", "remove-thread", "resolve", "dismiss", "staff-post", "spoiler", "unspoiler", "undead", "unundead"] {
            sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(1,$1,$2,$3)")
                .bind(&board).bind(ids[0]).bind(action).execute(&staff).await.unwrap();
        }
        sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask) VALUES(1,$1,$2,'thread-options',0,31)")
            .bind(&board).bind(ids[0]).execute(&staff).await.unwrap();
        for (action, before, after) in [
            ("thread-options", None, None),
            ("thread-options", Some(0_i16), None),
            ("thread-options", None, Some(1)),
            ("thread-options", Some(1), Some(1)),
            ("thread-options", Some(-1), Some(1)),
            ("thread-options", Some(0), Some(32)),
            ("close", Some(0), Some(1)),
            ("close", None, Some(1)),
            ("invented-action", None, None),
        ] {
            rejected(sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask) VALUES(1,$1,$2,$3,$4,$5)")
                .bind(&board).bind(ids[0]).bind(action).bind(before).bind(after).execute(&staff).await, "23514");
        }
        let historical: i64 = sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE board=$1 AND action<>'thread-options' AND before_mask IS NULL AND after_mask IS NULL")
            .bind(&board).fetch_one(&staff).await.unwrap();
        assert_eq!(historical, 18);

        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&board).execute(&owner).await.unwrap();
        for query in [
            "SELECT count(*) FROM content.threads WHERE board=$1",
            "SELECT count(*) FROM content.visible_threads WHERE board=$1",
        ] {
            let count: i64 = sqlx::query_scalar(query)
                .bind(&board).fetch_one(&public).await.unwrap();
            assert_eq!(count, 0);
        }
        assert!(board_store::threads(&public, &board, 0, 100).await.unwrap().is_empty());
        assert!(matches!(board_store::thread_statistics(&public, &board, ids[2]).await, Err(StoreError::NotFound)));
        assert_eq!(board_store::threads(&staff, &board, 0, 100).await.unwrap().len(), 4);
        let barrier: bool = sqlx::query_scalar("SELECT 'security_barrier=true'=ANY(reloptions) FROM pg_class WHERE oid='content.visible_threads'::regclass")
            .fetch_one(&owner).await.unwrap();
        assert!(barrier);
    }).await;
    for query in [
        "DELETE FROM content.moderation_audit WHERE board=$1",
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
    result.unwrap();
}
