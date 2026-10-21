#![cfg(feature = "database-tests")]
mod support;

use board_store::{NewPost, StoreError};
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
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES(false,$1,'Archive storage','Owned synthetic',2000,1000,300,1,1,864000,1,0,0,0,100)")
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

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned rollover".into(),
        comment: support::distinct_comment("Synthetic archive storage"),
        deletion_hash: "owned-fixture".into(),
        sage: false,
    }
}

#[tokio::test]
async fn storage_json_and_html_have_separate_source_rules() {
    let (owner, public, board) = fixture().await;
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let ids = seed(&o, &b, 3002).await;
        let json = board_store::archive_snapshot(&p, &b).await.unwrap();
        assert_eq!(json.entries.iter().map(|row| row.id).collect::<Vec<_>>(), ids);
        let html = board_store::archive_page_snapshot(&p, &b).await.unwrap();
        assert_eq!(html.snapshot.entries.len(), 3000);
        assert!(!html.snapshot.entries.iter().any(|row| row.id == ids[0]));
        assert!(html.snapshot.entries.windows(2).all(|rows| rows[0].id > rows[1].id));
        let first = support::create_post(&p, &b, 0, &post()).await.unwrap();
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 3002,
            "a new OP must not trim unexpired archives to archive_limit");
        sqlx::query("UPDATE content.threads SET bumped_at=clock_timestamp()-interval '90 hours',modified_at=to_timestamp(1000000000) WHERE board=$1 AND id=$2")
            .bind(&b).bind(first).execute(&o).await.unwrap();
        support::create_post(&p, &b, 0, &post()).await.unwrap();
        let archived = board_store::thread(&p, &b, first).await.unwrap();
        assert_eq!(Some(archived.bumped_at), archived.archived_at);
        assert_eq!(archived.modified_at.timestamp(), 1000000000,
            "source archive_thread changes root but preserves last_modified");
        let html = board_store::archive_page_snapshot(&p, &b).await.unwrap();
        assert_eq!(html.snapshot.entries[0].id, first,
            "a previously neglected thread becomes a recent archive");
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        support::create_post(&p, &b, 0, &post()).await.unwrap();
        assert!(matches!(board_store::archive_snapshot(&p, &b).await, Err(StoreError::NotFound)));
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.threads WHERE board=$1 AND archived_at IS NOT NULL AND NOT deleted").bind(&b).fetch_one(&o).await.unwrap(), 3003,
            "disabling archives must not delete their stored rows");
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=864000 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 3003);
    }).await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}

#[tokio::test]
async fn oversized_complete_archive_metadata_is_rejected_without_a_partial_list() {
    let (owner, public, board) = fixture().await;
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let ids = seed(&o, &b, board_store::MAX_ARCHIVE_JSON_ENTRIES as i64 + 1).await;
        let started = std::time::Instant::now();
        let error = board_store::archive_snapshot(&p, &b)
            .await
            .err()
            .expect("oversized metadata must fail");
        assert!(
            matches!(error, StoreError::ReadLimit),
            "unexpected reader error after {:?}: {error}",
            started.elapsed()
        );
        eprintln!(
            "Complete archive cardinality guard: {:?}",
            started.elapsed()
        );
        let last = *ids.last().unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(&b)
            .bind(last)
            .execute(&o)
            .await
            .unwrap();
        let complete = board_store::archive_snapshot(&p, &b).await.unwrap();
        assert_eq!(
            complete.entries.len(),
            board_store::MAX_ARCHIVE_JSON_ENTRIES
        );
        assert_eq!(complete.entries.first().unwrap().id, ids[0]);
        assert_eq!(complete.entries.last().unwrap().id, ids[ids.len() - 2]);
        assert_eq!(
            complete.entries.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            ids[..ids.len() - 1],
            "the complete projection preserves every retained ID in source order"
        );
        assert!(complete.entries.iter().all(|entry| entry.subject == "Owned archive"));
        sqlx::query(
            "UPDATE content.posts SET subject=repeat('x',100) WHERE board=$1 AND NOT deleted",
        )
        .bind(&b)
        .execute(&o)
        .await
        .unwrap();
        // Record the real public-role planner choice after the large UPDATE.
        // EXPLAIN does not execute the query or change its statement timeout.
        let timeout: String = sqlx::query_scalar("SHOW statement_timeout").fetch_one(&p).await.unwrap();
        assert_eq!(timeout, "5s", "archive diagnostics must retain the public limit");
        let plan = sqlx::query_scalar::<_, String>("EXPLAIN (VERBOSE, COSTS, SETTINGS) SELECT count(*),coalesce(sum(subject_bytes),0)::bigint FROM (SELECT octet_length(p.subject)::bigint AS subject_bytes FROM content.visible_threads t JOIN LATERAL (SELECT p.subject,p.deleted FROM content.posts p WHERE p.board=t.board AND p.id=t.id LIMIT 1) p ON true WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted LIMIT $2) selected")
            .bind(&b).bind(board_store::MAX_ARCHIVE_JSON_ENTRIES as i64 + 1).fetch_all(&p).await;
        eprintln!("Owned archive post-update public plan: {plan:#?}");
        let statistics = sqlx::query_scalar::<_,String>("SELECT row_to_json(s)::text FROM (SELECT relname,n_live_tup,n_dead_tup,analyze_count,autoanalyze_count,vacuum_count,autovacuum_count FROM pg_stat_all_tables WHERE schemaname='content' AND relname IN('posts','threads','boards') ORDER BY relname) s").fetch_all(&o).await;
        eprintln!("Owned archive post-update statistics: {statistics:#?}");
        let started = std::time::Instant::now();
        let result = board_store::archive_snapshot(&p, &b).await.err();
        assert!(
            matches!(result, Some(StoreError::ReadLimit)),
            "the complete subject projection must fit the existing byte budget; after {:?}: {result:?}",
            started.elapsed()
        );
    })
    .await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}
