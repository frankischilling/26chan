#![cfg(feature = "database-tests")]
use board_store::homepage::{self, RefreshError};
use sqlx::{Connection, PgConnection, PgPool};
async fn op(owner: &PgPool, board: &str, subject: &str) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO content.threads(board,bumped_at) VALUES($1,clock_timestamp()) RETURNING id",
    )
    .bind(board)
    .fetch_one(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$1,'Anonymous',$3,'Owned public frontpage body',clock_timestamp()-interval '12 minutes')").bind(id).bind(board).bind(subject).execute(owner).await.unwrap();
    id
}
async fn image(owner: &PgPool, post: i64, filename: &str) -> (String, String) {
    let (job, asset): (String, String) = sqlx::query_as(
        "SELECT replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-','')",
    )
    .fetch_one(owner)
    .await
    .unwrap();
    let lease = "d".repeat(32);
    sqlx::query("INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes) VALUES($1,$2,'published',100,1,$3,repeat('a',64),100)").bind(&job).bind(filename).bind(&lease).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES($1,$2,$3,repeat('b',64),100,10,10,'approved',clock_timestamp())").bind(&asset).bind(&job).bind(&lease).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$3,$4,100,10,10,false)").bind(post).bind(&job).bind(&asset).bind(filename).execute(owner).await.unwrap();
    (job, asset)
}
#[tokio::test]
async fn snapshots_keep_source_candidate_order_and_recheck_current_public_state() {
    let owner_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let owner = PgPool::connect(&owner_url).await.unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(sqlx::query_scalar::<_,bool>("SELECT NOT EXISTS(SELECT 1 FROM site_private.frontpage_state) AND NOT EXISTS(SELECT 1 FROM site_private.frontpage_cards) AND NOT EXISTS(SELECT 1 FROM site_private.board_statistics)").fetch_one(&owner).await.unwrap(),"requires fresh disposable snapshot tables");
    let board = format!(
        "hp{:06x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
            & 0xffffff
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned homepage board','',1000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(homepage::READINESS_SQL)
            .fetch_one(&public)
            .await
            .unwrap()
    );
    for drift in [
        "ALTER VIEW content.homepage_cards SET (security_barrier=false)",
        "GRANT SELECT ON site_private.frontpage_state TO board_public",
        "GRANT SELECT(post_id) ON site_private.frontpage_cards TO board_public",
        "GRANT SELECT ON site_private.frontpage_eligible TO board_public",
        "GRANT UPDATE ON content.homepage_statistics TO board_public",
        "ALTER TABLE site_private.frontpage_cards DROP CONSTRAINT frontpage_cards_slot_check",
        "ALTER TABLE site_private.board_statistics DROP CONSTRAINT board_statistics_slot_check",
        "CREATE OR REPLACE VIEW content.homepage_statistics WITH(security_barrier=true) AS SELECT generation,generated_at,recorded_posts,active_content_bytes,active_sessions FROM site_private.frontpage_state",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(drift).execute(&mut *tx).await.unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>(homepage::READINESS_SQL)
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            "accepted drift: {drift}"
        );
        tx.rollback().await.unwrap();
    }
    // Literal whitespace is part of the source marker, not SQL formatting.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query(r#"DO $$ DECLARE original text; changed text; BEGIN
        original := pg_get_viewdef('site_private.frontpage_eligible'::regclass,true);
        changed := replace(original,'''SPOILER<>''::text','''SPOILER <>''::text');
        IF original=changed THEN RAISE EXCEPTION 'owned marker fixture is missing'; END IF;
        EXECUTE 'CREATE OR REPLACE VIEW site_private.frontpage_eligible WITH(security_barrier=true) AS ' || changed;
    END $$;"#).execute(&mut *tx).await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>(homepage::READINESS_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    );
    tx.rollback().await.unwrap();
    let main = op(&owner, &board, "Owned popular thread").await;
    sqlx::query("UPDATE content.posts SET staff_notices=ARRAY['Owned notice <literal>','Owned repeated','Owned repeated'] WHERE id=$1")
        .bind(main).execute(&owner).await.unwrap();
    let picture = op(&owner, &board, "Owned image").await;
    let blocked = op(&owner, &board, "Owned excluded filename").await;
    let excluded = op(&owner, "b", "Owned excluded source board").await;
    let media = [
        image(&owner, picture, "owned.png").await,
        image(&owner, blocked, "owned.pdf.png").await,
    ];
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let board=board.clone();async move {
        let mut writer=PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await.unwrap();
        let before=homepage::snapshot(&public).await.unwrap();assert!(before.statistics.is_none());assert!(before.cards.is_empty());
        sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment,created_at) SELECT $1,$2,'Anonymous','','Owned healthy reply',clock_timestamp()-interval '11 minutes' FROM generate_series(1,15)").bind(&board).bind(main).execute(&owner).await.unwrap();
        // Deliberately leave the cached thread counter at zero: source popularity counts actual rows.
        assert_eq!(homepage::refresh(&mut writer).await.unwrap(),1);
        let snap=homepage::snapshot(&public).await.unwrap();
        assert!(snap.statistics.as_ref().unwrap().recorded_posts>=19);
        assert_eq!(snap.cards.iter().filter(|c|c.kind=="latest" && c.board==board).count(),5);
        assert!(snap.cards.iter().any(|c|c.kind=="popular" && c.id==main && c.reply_count==15));
        let notice_card=snap.cards.iter().find(|c|c.kind=="popular" && c.id==main).unwrap();
        assert!(notice_card.excerpt.ends_with("\n\n(Owned notice <literal>)\n\n(Owned repeated)\n\n(Owned repeated)"));
        assert!(snap.cards.iter().any(|c|c.kind=="images" && c.id==picture));
        assert!(!snap.cards.iter().any(|c|c.id==blocked || c.id==excluded));
        assert_eq!(homepage::board_statistics(&public,Some(&board)).await.unwrap()[0].recorded_posts,18);
        sqlx::query("UPDATE content.threads SET sticky=true,bumped_at=clock_timestamp()-interval '20 minutes' WHERE id=$1").bind(main).execute(&owner).await.unwrap();
        homepage::refresh(&mut writer).await.unwrap();
        assert!(homepage::snapshot(&public).await.unwrap().cards.iter().any(|c|c.kind=="popular" && c.id==main),"source sticky roots are encoded in the future");
        sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1").bind(main).execute(&owner).await.unwrap();
        homepage::refresh(&mut writer).await.unwrap();
        assert!(!homepage::snapshot(&public).await.unwrap().cards.iter().any(|c|c.kind=="popular" && c.id==main));
        sqlx::query("UPDATE content.threads SET bumped_at=clock_timestamp() WHERE id=$1").bind(main).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment,created_at) SELECT $1,$2,'Anonymous','','[spoiler]Owned hidden reply[/spoiler]',clock_timestamp()-interval '601 seconds' FROM generate_series(1,5)").bind(&board).bind(main).execute(&owner).await.unwrap();
        homepage::refresh(&mut writer).await.unwrap();
        assert_eq!(homepage::snapshot(&public).await.unwrap().cards.iter().filter(|c|c.kind=="latest" && c.board==board).count(),0,"do not fill source's five filtered candidates with older posts");
        let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned homepage report') RETURNING id").bind(&board).bind(main).fetch_one(&owner).await.unwrap();
        assert!(!homepage::snapshot(&public).await.unwrap().cards.iter().any(|c|c.id==main),"new reports remove cached cards immediately");
        sqlx::query("DELETE FROM content.reports WHERE id=$1 AND reason='Owned homepage report'").bind(report).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.post_media SET file_deleted=true WHERE post_id=$1").bind(picture).execute(&owner).await.unwrap();
        assert!(!homepage::snapshot(&public).await.unwrap().cards.iter().any(|c|c.kind=="images" && c.id==picture));
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        let snap=homepage::snapshot(&public).await.unwrap();assert!(snap.statistics.is_none());assert!(!snap.cards.iter().any(|c|c.board==board));
        assert!(homepage::board_statistics(&public,Some(&board)).await.unwrap().is_empty());
        sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        for _ in 0..64 {homepage::refresh(&mut writer).await.unwrap();}
        assert_eq!(homepage::board_statistics(&public,Some(&board)).await.unwrap().len(),64);
        sqlx::query("UPDATE site_private.frontpage_state SET generated_at=clock_timestamp()-interval '16 minutes'").execute(&owner).await.unwrap();
        assert!(homepage::snapshot(&public).await.unwrap().statistics.unwrap().stale());
        let previous:i64=sqlx::query_scalar("SELECT count(*) FROM site_private.frontpage_cards").fetch_one(&owner).await.unwrap();
        let before=homepage::snapshot(&public).await.unwrap().statistics.unwrap().generation;
        let mut second=PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await.unwrap();
        let (a,b)=tokio::join!(homepage::refresh(&mut writer),homepage::refresh(&mut second));
        let successful=i64::from(a.is_ok())+i64::from(b.is_ok());
        assert!(successful>=1,"at least one concurrent operator refresh commits");
        assert!(a.is_ok() || matches!(a,Err(RefreshError::Database)));
        assert!(b.is_ok() || matches!(b,Err(RefreshError::Database)));
        assert_eq!(homepage::snapshot(&public).await.unwrap().statistics.unwrap().generation,before+successful);
        sqlx::query("UPDATE site_private.frontpage_state SET generation=2147483647").execute(&owner).await.unwrap();
        assert!(matches!(homepage::refresh(&mut writer).await,Err(RefreshError::Generation)));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM site_private.frontpage_cards").fetch_one(&owner).await.unwrap(),previous);
        let mut reader=PgConnection::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
        assert!(matches!(homepage::refresh(&mut reader).await,Err(RefreshError::Role)));
        for sql in ["SELECT * FROM site_private.frontpage_cards","SELECT * FROM site_private.frontpage_state","SELECT * FROM site_private.frontpage_eligible"] {
            let error=sqlx::query(sql).execute(&public).await.unwrap_err();assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("42501"));
        }
        assert!(!sqlx::query_scalar::<_,bool>("SELECT has_table_privilege(current_user,'content.homepage_cards','UPDATE')").fetch_one(&public).await.unwrap());
        let error=sqlx::query("UPDATE content.homepage_cards SET slot=1").execute(&public).await.unwrap_err();
        assert!(matches!(error.as_database_error().unwrap().code().as_deref(),Some("42501"|"55000")));
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(main).execute(&owner).await.unwrap();
        assert!(!homepage::snapshot(&public).await.unwrap().cards.iter().any(|c|c.id==main));
    }}).await;
    sqlx::raw_sql("DELETE FROM site_private.frontpage_cards; DELETE FROM site_private.board_statistics; DELETE FROM site_private.frontpage_state").execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=$1 AND reason='Owned homepage report'")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.post_media WHERE post_id IN ($1,$2)")
        .bind(picture)
        .bind(blocked)
        .execute(&owner)
        .await
        .unwrap();
    for (job, asset) in media {
        sqlx::query("DELETE FROM media.assets WHERE id=$1")
            .bind(asset)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM media.jobs WHERE id=$1")
            .bind(job)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM content.posts WHERE board=$1 OR (board='b' AND id=$2)")
        .bind(&board)
        .bind(excluded)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1 OR (board='b' AND id=$2)")
        .bind(&board)
        .bind(excluded)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1 AND title='Owned homepage board'")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
