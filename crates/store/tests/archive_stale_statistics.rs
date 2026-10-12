#![cfg(feature = "database-tests")]

use board_store::StoreError;
use sqlx::PgPool;

async fn seed(owner: &PgPool, board: &str, count: i64) -> Vec<i64> {
    let mut ids: Vec<i64> = sqlx::query_scalar("WITH inserted AS (INSERT INTO content.threads(board,archived_at,archive_expires_at,bumped_at) SELECT $1,CASE WHEN n=1 THEN statement_timestamp()-interval '73 hours' ELSE statement_timestamp() END,statement_timestamp()+interval '5 days',CASE WHEN n=1 THEN statement_timestamp()-interval '73 hours' ELSE statement_timestamp() END FROM generate_series(1,$2::bigint) n RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Anonymous','Owned archive','Synthetic stored archive' FROM inserted RETURNING id")
        .bind(board).bind(count).fetch_all(owner).await.unwrap();
    ids.sort_unstable();
    ids
}

async fn fixture() -> (PgPool, PgPool, String) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'as'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Archive storage','Owned synthetic',2000,1000,300,1,1,864000,1,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    (owner, public, board)
}

async fn cleanup(owner: &PgPool, public: &PgPool, board: &str) {
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query).bind(board).execute(owner).await.unwrap();
    }
    public.close().await;
    owner.close().await;
}

// Keep this fixture in its own test binary: ANALYZE deliberately leaves statistics
// for an old board before a new board receives a large archive batch.
#[tokio::test]
async fn complete_archive_reads_survive_stale_board_statistics() {
    let (owner, public, old_board) = fixture().await;
    seed(&owner, &old_board, 3002).await;
    for query in [
        "ANALYZE content.threads",
        "ANALYZE content.posts",
        "ANALYZE content.boards",
    ] {
        sqlx::query(query).execute(&owner).await.unwrap();
    }
    cleanup(&owner, &public, &old_board).await;

    let (owner, public, board) = fixture().await;
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let ids = seed(&o, &b, 10001).await;
        let snapshot = board_store::archive_snapshot(&p, &b).await.unwrap();
        assert_eq!(
            snapshot
                .entries
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            ids
        );
        let html = board_store::archive_page_snapshot(&p, &b).await.unwrap();
        assert_eq!(html.snapshot.entries.len(), 3000);
        assert_eq!(
            html.snapshot.entries.first().unwrap().id,
            *ids.last().unwrap()
        );
        assert!(!html.snapshot.entries.iter().any(|row| row.id == ids[0]));
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&b)
            .execute(&o)
            .await
            .unwrap();
        assert!(matches!(
            board_store::archive_snapshot(&p, &b).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            board_store::archive_page_snapshot(&p, &b).await,
            Err(StoreError::NotFound)
        ));
    })
    .await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}
