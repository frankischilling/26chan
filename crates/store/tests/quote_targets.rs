#![cfg(feature = "database-tests")]
mod support;

use board_domain::post_quote::QuoteTargetKey;
use board_store::{NewPost, Post, QuoteTargets};
use sqlx::{PgPool, Postgres, Transaction};

fn new_post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned quote target".into(),
        deletion_hash: "owned-fixture-password-hash".into(),
        sage: false,
    }
}
fn request(mut post: Post, board: &str, ids: &[i64]) -> Post {
    post.board = board.into();
    post.comment = ids.iter().map(|id| format!(">>{id} ")).collect();
    post.comment_format = 104;
    post.wordfilter_payload = None;
    post
}
async fn snapshot(pool: &PgPool) -> Transaction<'static, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}
fn lookup(targets: &QuoteTargets, board: &str, id: i64) -> Option<i64> {
    targets.thread_id(&QuoteTargetKey::new(board, id).unwrap())
}

#[tokio::test]
async fn paired_identity_only_lookup_honors_visibility_and_repeatable_read() {
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
    let boards = vec![format!("qa{seed:x}"), format!("qb{seed:x}")];
    for board in &boards {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,archive_retention_seconds) VALUES($1,'Owned quote identities','Synthetic visibility fixture',16000,1000,1000,1000,10,0,0,0,100,3600)")
            .bind(board).execute(&owner).await.unwrap();
    }
    let test_boards = boards.clone();
    let test_owner = owner.clone();
    let result=tokio::spawn(async move {
        let a=&test_boards[0]; let b=&test_boards[1];
        let op=support::create_post(&public,a,0,&new_post()).await.unwrap();
        let reply=support::create_post(&public,a,op,&new_post()).await.unwrap();
        let expired=support::create_post(&public,a,0,&new_post()).await.unwrap();
        let other=support::create_post(&public,b,0,&new_post()).await.unwrap();
        let base=board_store::find_post(&public,a,op).await.unwrap();
        let references=request(base.clone(),a,&[op,reply,expired]);
        let mut tx=snapshot(&public).await;
        let targets=board_store::quote_targets::load(&mut tx,std::slice::from_ref(&references)).await.unwrap();
        assert_eq!(lookup(&targets,a,op),Some(op));
        assert_eq!(lookup(&targets,a,reply),Some(op));
        assert!(targets.has_dependencies());
        tx.commit().await.unwrap();

        // Independent ANY arrays would incorrectly include both existing rows.
        let mut tx=snapshot(&public).await;
        let targets=board_store::quote_targets::load(&mut tx,&[request(base.clone(),a,&[other]),request(base.clone(),b,&[op])]).await.unwrap();
        for (board,id) in [(a,op),(a,other),(b,op),(b,other)] {
            assert_eq!(lookup(&targets,board,id),None,"only exact requested pairs may return");
        }
        tx.commit().await.unwrap();

        // Fix the public read snapshot before another connection deletes a reply.
        let mut tx=snapshot(&public).await;
        let saved:bool=sqlx::query_scalar("SELECT deleted FROM content.posts WHERE board=$1 AND id=$2")
            .bind(a).bind(reply).fetch_one(&mut *tx).await.unwrap();
        assert!(!saved);
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(a).bind(reply).execute(&test_owner).await.unwrap();
        let targets=board_store::quote_targets::load(&mut tx,std::slice::from_ref(&references)).await.unwrap();
        assert_eq!(lookup(&targets,a,reply),Some(op),"same snapshot retains old target identity");
        tx.commit().await.unwrap();

        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE board=$1 AND id=$2")
            .bind(a).bind(expired).execute(&test_owner).await.unwrap();
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(b).execute(&test_owner).await.unwrap();
        let requests=[references,request(base.clone(),b,&[other])];
        let mut tx=snapshot(&public).await;
        let targets=board_store::quote_targets::load(&mut tx,&requests).await.unwrap();
        assert_eq!(lookup(&targets,a,op),Some(op));
        assert_eq!(lookup(&targets,a,reply),None,"deleted target");
        assert_eq!(lookup(&targets,a,expired),None,"expired parent");
        assert_eq!(lookup(&targets,b,other),None,"private board");
        tx.commit().await.unwrap();
        // Explicit staff_only predicate also holds under an owner connection.
        let mut tx=snapshot(&test_owner).await;
        let targets=board_store::quote_targets::load(&mut tx,&requests).await.unwrap();
        assert_eq!(lookup(&targets,b,other),None);
        tx.commit().await.unwrap();
        // A live reply under a deleted OP has no navigable public thread.
        sqlx::query("UPDATE content.posts SET deleted=false WHERE board=$1 AND id=$2")
            .bind(a).bind(reply).execute(&test_owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(a).bind(op).execute(&test_owner).await.unwrap();
        let mut tx=snapshot(&public).await;
        let targets=board_store::quote_targets::load(&mut tx,&requests).await.unwrap();
        assert_eq!(lookup(&targets,a,reply),None,"deleted OP makes its live reply unavailable");
        tx.commit().await.unwrap();
        sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1 AND id=$2")
            .bind(a).bind(op).execute(&test_owner).await.unwrap();
        let mut tx=snapshot(&public).await;
        let targets=board_store::quote_targets::load(&mut tx,&requests).await.unwrap();
        assert_eq!(lookup(&targets,a,op),None,"deleted parent");
        tx.commit().await.unwrap();
    }).await;
    let mut cleanup = support::begin_cleanup(&owner, &boards).await;
    for board in &boards {
        support::cleanup_posting(&mut *cleanup, board).await;
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
            .bind(board).execute(&mut *cleanup).await.unwrap();
        sqlx::query("DELETE FROM content.posts WHERE board=$1")
            .bind(board)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=$1")
            .bind(board)
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(board)
            .execute(&mut *cleanup)
            .await
            .unwrap();
    }
    cleanup.commit().await.unwrap();
    result.unwrap();
}
