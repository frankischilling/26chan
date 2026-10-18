#![cfg(feature = "database-tests")]
use board_domain::content_admission::classifiers::posting::{self, history};
use board_store::posting_classifiers::diagnostics::{self, Event, Written};
use sqlx::PgPool;

fn code(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned())
}
#[tokio::test]
async fn private_diagnostics_are_ordered_bounded_and_separate_from_cooldowns() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'dg'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned diagnostics','Synthetic',true,2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let events = [Event::Prefix(posting::Event::MaskChanged),
            Event::History(history::Event::RecordCountryChange),
            Event::History(history::Event::LogCountryBlock)];
        for commit in [false,true] {
            let mut tx = p.begin().await.unwrap();
            assert_eq!(diagnostics::record(&mut tx,&b,0,&events,Some("US"),None).await.unwrap(),Written::Recorded);
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.classifier_events WHERE board=$1").bind(&b).fetch_one(&o).await.unwrap();
            assert_eq!(count,0);
            if commit {tx.commit().await.unwrap();} else {tx.rollback().await.unwrap();}
        }
        let saved: Vec<(String,i64,Option<String>,Option<f64>)> = sqlx::query_as("SELECT kind,thread_id,country,threat_score FROM post_secrets.classifier_events WHERE board=$1 ORDER BY id")
            .bind(&b).fetch_all(&o).await.unwrap();
        assert_eq!(saved,vec![("log_mask_changed".into(),0,None,None),("country_changed".into(),0,Some("US".into()),None),("block_country_changed".into(),0,None,None)]);
        for query in ["SELECT * FROM post_secrets.classifier_events", "SELECT content.expire_classifier_events()", "SELECT post_secrets.prune_classifier_events()"] {
            assert_eq!(code(&sqlx::raw_sql(query).execute(&p).await.unwrap_err()).as_deref(),Some("42501"));
        }
        for kinds in [vec![],vec!["not-an-event"],vec!["log_mask_changed";4],vec!["country_changed"],vec!["block_txt_threat","block_country_changed"],vec!["log_mask_changed","log_mask_changed"]] {
            let error = sqlx::query("SELECT content.record_posting_classifier_events($1,0,$2,NULL,NULL)")
                .bind(&b).bind(kinds).execute(&p).await.unwrap_err();
            assert_eq!(code(&error).as_deref(),Some("23514"));
        }
        sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        let error = sqlx::query("SELECT content.record_posting_classifier_events($1,0,ARRAY['log_mask_changed'],NULL,NULL)").bind(&b).execute(&p).await.unwrap_err();
        assert_eq!(code(&error).as_deref(),Some("28000"));
        sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1").bind(&b).execute(&o).await.unwrap();
        sqlx::query("GRANT SELECT ON post_secrets.classifier_events TO board_public").execute(&o).await.unwrap();
        let drift = sqlx::query("SELECT content.record_posting_classifier_events($1,0,ARRAY['log_mask_changed'],NULL,NULL)").bind(&b).execute(&p).await;
        sqlx::query("REVOKE SELECT ON post_secrets.classifier_events FROM board_public").execute(&o).await.unwrap();
        assert_eq!(code(&drift.unwrap_err()).as_deref(),Some("P199U"));
        sqlx::query("INSERT INTO post_secrets.classifier_events(created_at,kind,board,thread_id) SELECT floor(extract(epoch FROM clock_timestamp()))::bigint-691200,'log_mask_changed',$1,0 FROM generate_series(1,1005)")
            .bind(&b).execute(&o).await.unwrap();
        for expected in [1000_i64,5,0] {
            let count: i64 = sqlx::query_scalar("SELECT content.expire_classifier_events()").fetch_one(&o).await.unwrap();
            assert_eq!(count,expected);
        }
        // The fresh bot event's formatted source metadata also contains the
        // model score. This is distinct from the omitted ingress bot score.
        let mut tx = p.begin().await.unwrap();
        let recorded: bool = sqlx::query_scalar("SELECT content.record_posting_classifier_events($1,42,ARRAY['block_bm_bot'],'US',0.25)").bind(&b).fetch_one(&mut *tx).await.unwrap();
        assert!(recorded);
        tx.commit().await.unwrap();
        let metadata: (Option<String>,Option<f64>) = sqlx::query_as("SELECT country,threat_score FROM post_secrets.classifier_events WHERE board=$1 AND kind='block_bm_bot'").bind(&b).fetch_one(&o).await.unwrap();
        assert_eq!(metadata,(None,Some(0.25)));
        sqlx::query("DELETE FROM post_secrets.classifier_events WHERE board=$1 AND kind='block_bm_bot'").bind(&b).execute(&o).await.unwrap();
        // Capacity is diagnostic loss, never a replacement admission outcome.
        sqlx::query("INSERT INTO post_secrets.classifier_events(created_at,kind,board,thread_id) SELECT floor(extract(epoch FROM clock_timestamp()))::bigint,'log_mask_changed',$1,0 FROM generate_series(1,99994)")
            .bind(&b).execute(&o).await.unwrap();
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let write = |pool: PgPool, board: String, barrier: std::sync::Arc<tokio::sync::Barrier>| async move {
            let mut tx = pool.begin().await.unwrap();
            barrier.wait().await;
            let written = diagnostics::record(&mut tx,&board,0,&events,Some("US"),None).await.unwrap();
            tx.commit().await.unwrap();
            written
        };
        let (first,second) = tokio::join!(write(p.clone(),b.clone(),barrier.clone()),write(p.clone(),b.clone(),barrier));
        assert!(matches!((first,second),(Written::Recorded,Written::CapacityDropped)|(Written::CapacityDropped,Written::Recorded)));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.classifier_events WHERE board=$1").bind(&b).fetch_one(&o).await.unwrap();
        assert_eq!(count,100000);
        let invalid = sqlx::query("SELECT content.record_posting_classifier_events($1,0,ARRAY['not-an-event'],NULL,NULL)").bind(&b).execute(&p).await.unwrap_err();
        assert_eq!(code(&invalid).as_deref(),Some("23514"));

        let content: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&b).fetch_one(&o).await.unwrap();
        assert_eq!(content,0);
    }).await;
    sqlx::query("DELETE FROM post_secrets.classifier_events WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
