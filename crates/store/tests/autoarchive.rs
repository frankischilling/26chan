#![cfg(feature = "database-tests")]
#[path = "../../../tests/helpers/restriction_catalog.rs"]
mod restriction_catalog;
mod support;
use board_store::{NewPost, StoreError};
use sqlx::PgPool;

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned autoarchive".into(),
        comment: support::distinct_comment("Synthetic reply"),
        deletion_hash: "fixture-hash".into(),
        sage: true,
    }
}

#[tokio::test]
async fn automatic_archive_threshold_and_preserved_reply() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'aa'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,autoarchive_cap,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES(false,$1,'Autoarchive fixture','Owned synthetic',2000,1000,1000,100,10,3600,3,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    let b = board.clone();
    let o = owner.clone();
    let p = public.clone();
    let result = tokio::spawn(async move {
        let op = support::create_post(&p,&b,0,&post()).await.unwrap();
        sqlx::query("UPDATE content.posts SET since4pass=2020 WHERE id=$1").bind(op).execute(&o).await.unwrap();
        let mut membership=o.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *membership).await.unwrap();
        sqlx::query("INSERT INTO verification_private.pass_posts VALUES($1,sha256('owned-archive-pass'))").bind(op).execute(&mut *membership).await.unwrap();membership.commit().await.unwrap();
        for _ in 0..2 { support::create_post(&p,&b,op,&post()).await.unwrap(); }
        assert!(board_store::thread(&p,&b,op).await.unwrap().archived_at.is_none());
        sqlx::query("UPDATE content.threads SET bumped_at=clock_timestamp()-interval '90 hours' WHERE board=$1 AND id=$2").bind(&b).bind(op).execute(&o).await.unwrap();
        let last = support::create_post(&p,&b,op,&post()).await.unwrap();
        let snapshot = board_store::thread_snapshot(&p,&b,op).await.unwrap();
        assert_eq!(snapshot.posts.len(),4);
        assert_eq!(snapshot.posts[0].since4pass,2020,"archive retains the public opt-in year");
        let mut membership=o.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *membership).await.unwrap();
        let private_pass:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=$1)").bind(op).fetch_one(&mut *membership).await.unwrap();assert!(!private_pass,"archive removes the private Pass identity");membership.rollback().await.unwrap();
        assert_eq!(snapshot.posts.last().unwrap().id,last);
        let archived = snapshot.thread.archived_at.unwrap();
        assert_eq!(snapshot.thread.bumped_at,archived);
        assert_eq!((snapshot.thread.archive_expires_at.unwrap()-archived).num_seconds(),3600);
        assert_eq!(board_store::archive_snapshot(&p,&b).await.unwrap().entries.len(),1);
        assert_eq!(board_store::archive_page_snapshot(&p,&b).await.unwrap().snapshot.entries.len(),1);
        let secrets: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board=$1 AND p.thread_id=$2").bind(&b).bind(op).fetch_one(&o).await.unwrap();
        assert_eq!(secrets,0);
        assert!(support::create_post(&p,&b,op,&post()).await.is_err());
        // Protected threads, disabled archives and zero caps do not transition.
        for (sticky,undead,enabled,cap) in [(true,false,true,1),(false,true,true,1),(true,true,true,1),(false,false,false,1),(false,false,true,0)] {
            sqlx::query("UPDATE content.boards SET archive_retention_seconds=$2,autoarchive_cap=$3 WHERE slug=$1").bind(&b).bind(if enabled {3600} else {0}).bind(cap).execute(&o).await.unwrap();
            let op = support::create_post(&p,&b,0,&post()).await.unwrap();
            sqlx::query("UPDATE content.threads SET sticky=$3,undead=$4 WHERE board=$1 AND id=$2").bind(&b).bind(op).bind(sticky).bind(undead).execute(&o).await.unwrap();
            support::create_post(&p,&b,op,&post()).await.unwrap();
            assert!(board_store::thread(&p,&b,op).await.unwrap().archived_at.is_none());
        }
        // Cached lifetime counts must not replace the source live-row count.
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600,autoarchive_cap=3 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        let live_op = support::create_post(&p,&b,0,&post()).await.unwrap();
        let deleted = support::create_post(&p,&b,live_op,&post()).await.unwrap();
        support::create_post(&p,&b,live_op,&post()).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2").bind(&b).bind(deleted).execute(&o).await.unwrap();
        support::create_post(&p,&b,live_op,&post()).await.unwrap();
        assert!(board_store::thread(&p,&b,live_op).await.unwrap().archived_at.is_none());
        let mut invalid=post(); invalid.comment="x".repeat(2001);
        assert!(support::create_post(&p,&b,live_op,&invalid).await.is_err());
        assert!(board_store::thread(&p,&b,live_op).await.unwrap().archived_at.is_none());
        support::create_post(&p,&b,live_op,&post()).await.unwrap();
        assert!(board_store::thread(&p,&b,live_op).await.unwrap().archived_at.is_some());
        // A late archive failure rolls back the accepted reply and its private
        // deletion state together. The synthetic trigger affects only this board.
        sqlx::query("UPDATE content.boards SET autoarchive_cap=1 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        let rollback_op = support::create_post(&p,&b,0,&post()).await.unwrap();
        assert!(b.len()==10 && b.bytes().all(|byte|byte.is_ascii_lowercase() || byte.is_ascii_digit()));
        let function = format!("content.autoarchive_fault_{b}");
        let original_catalog = restriction_catalog::snapshot(&o).await;
        restriction_catalog::apply(&o, &format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Owned archive failure' USING ERRCODE='ZX001'; END $$; CREATE TRIGGER autoarchive_fault_{b} BEFORE UPDATE OF archived_at ON content.threads FOR EACH ROW WHEN(NEW.board='{b}' AND OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL) EXECUTE FUNCTION {function}();"), None).await;
        let failed = support::create_post(&p,&b,rollback_op,&post()).await;
        restriction_catalog::apply(&o, &format!("DROP TRIGGER autoarchive_fault_{b} ON content.threads; DROP FUNCTION {function}();"), Some(&original_catalog)).await;
        assert!(matches!(failed,Err(StoreError::Database(ref error)) if error.as_database_error().and_then(|e| e.code()).as_deref()==Some("ZX001")), "Unexpected owned fault result: {failed:?}");
        let unchanged=board_store::thread_snapshot(&p,&b,rollback_op).await.unwrap();
        assert_eq!(unchanged.posts.len(),1);
        assert!(unchanged.thread.archived_at.is_none());
        let hashes:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board=$1 AND p.thread_id=$2").bind(&b).bind(rollback_op).fetch_one(&o).await.unwrap();
        assert_eq!(hashes,1);
        // The board lock lets only one of two simultaneous final replies commit.
        sqlx::query("UPDATE content.boards SET autoarchive_cap=1 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        let concurrent = support::create_post(&p,&b,0,&post()).await.unwrap();
        let message=post();
        let (a,z)=tokio::join!(support::create_post(&p,&b,concurrent,&message),support::create_post(&p,&b,concurrent,&message));
        assert_ne!(a.is_ok(),z.is_ok());
        let snapshot=board_store::thread_snapshot(&p,&b,concurrent).await.unwrap();
        assert_eq!(snapshot.posts.len(),2);
        assert!(snapshot.thread.archived_at.is_some());
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600,autoarchive_cap=1500 WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        let op = support::create_post(&p,&b,0,&post()).await.unwrap();
        sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) SELECT $1,$2,'Anonymous','','Historical reply' FROM generate_series(1,1499)").bind(&b).bind(op).execute(&o).await.unwrap();
        sqlx::query("UPDATE content.threads SET reply_count=1499 WHERE board=$1 AND id=$2").bind(&b).bind(op).execute(&o).await.unwrap();
        let last = support::create_post(&p,&b,op,&post()).await.unwrap();
        let snapshot = board_store::thread_snapshot(&p,&b,op).await.unwrap();
        assert_eq!(snapshot.posts.len(),1501);
        assert_eq!(snapshot.posts.last().unwrap().id,last);
        assert!(snapshot.thread.archived_at.is_some());
        // SQLx reuses prepared statements. Exercise the generic plan as well
        // as the default planner, with the real public role and its 5s limit.
        let generic = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .after_connect(|connection, _| Box::pin(async move {
                sqlx::query("SET plan_cache_mode=force_generic_plan")
                    .execute(connection).await?;
                Ok(())
            }))
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await.unwrap();
        let (role, timeout): (String, String) = sqlx::query_as("SELECT current_user::text,current_setting('statement_timeout')")
            .fetch_one(&generic).await.unwrap();
        assert_eq!(role,"board_public");
        assert_eq!(timeout,"5s");
        let generic_snapshot = board_store::thread_snapshot(&generic,&b,op).await.unwrap();
        assert_eq!(generic_snapshot.posts.iter().map(|post| post.id).collect::<Vec<_>>(),snapshot.posts.iter().map(|post| post.id).collect::<Vec<_>>());
        assert!(generic_snapshot.posts.iter().all(|post| post.attachment.is_none()));
        assert_eq!(generic_snapshot.thread.archived_at,snapshot.thread.archived_at);
        generic.close().await;
        assert!(matches!(support::create_post(&p,&b,op,&post()).await,Err(StoreError::Conflict(_))|Err(StoreError::NotFound)));
    }).await;
    let mut tx = support::begin_cleanup(&owner, std::slice::from_ref(&board)).await;
    support::cleanup_posting(&mut *tx, &board).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    public.close().await;
    owner.close().await;
    result.unwrap();
}
