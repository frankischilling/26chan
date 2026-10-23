#![cfg(feature = "database-tests")]
use board_domain::site_submissions::contests::Kind;
use board_store::site_submissions::contests;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
#[tokio::test]
async fn concurrent_schedules_and_approved_pages_keep_contacts_and_live_scores_private() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let work=tokio::spawn({let owner=owner.clone();let public=public.clone();let ids=ids.clone();async move {
        for _ in 0..20 {
            let id:i64=sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(&owner).await.unwrap();
            sqlx::query("INSERT INTO submission_private.contest_campaigns(id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity) VALUES($1,1,true,ARRAY['g'],1,2,2,3,100,100)").bind(id).execute(&owner).await.unwrap();ids.lock().unwrap().push(id);
        }
        let first=contests::directory(&public,Kind::BoardBanner,0).await.unwrap();assert_eq!(first.campaigns.len(),16);
        let second=contests::directory(&public,Kind::BoardBanner,first.next.unwrap()).await.unwrap();assert_eq!(second.campaigns.len(),4);assert!(second.next.is_none());
        let campaign=first.campaigns[0].id;
        assert_eq!(board_store::site_submissions::contest_profile(&public,Kind::BoardBanner).await.unwrap().unwrap().campaign.id,campaign);
        let mut sorted=Vec::new();
        for index in 0..29_i32 {
            let entry:i64=sqlx::query_scalar("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,email,media_id,state,score,created_at,updated_at) VALUES($1,'g',$2,$2,'Private credit','private@example.org',$3,$4,1,1,1) RETURNING id")
                .bind(campaign).bind(vec![index as u8;32]).bind(format!("{index:032x}")).bind(if index<27 {1_i16}else{0}).fetch_one(&owner).await.unwrap();if index<27{sorted.push(entry);}
        }
        sorted.reverse();let first=contests::listing(&public,campaign,"g",0).await.unwrap().unwrap();assert!(first.visible&&!first.voting);assert_eq!(first.next,Some(25));
        assert_eq!(first.entries.iter().map(|e|e.id).collect::<Vec<_>>(),sorted[..25]);assert!(first.entries.iter().all(|e|e.score==Some(1)));
        let last=contests::listing(&public,campaign,"g",25).await.unwrap().unwrap();assert_eq!(last.entries.iter().map(|e|e.id).collect::<Vec<_>>(),sorted[25..]);assert!(last.next.is_none());
        assert!(contests::listing(&public,campaign,"missing",0).await.unwrap().is_none());assert!(contests::listing(&public,campaign,"g",100001).await.is_err());
        sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=253402300799 WHERE id=$1").bind(campaign).execute(&owner).await.unwrap();
        assert!(contests::listing(&public,campaign,"g",0).await.unwrap().unwrap().entries.iter().all(|e|e.score.is_none()));
        let vt:i64=sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.contest_campaigns(id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity) VALUES($1,2,true,ARRAY['all'],1,2,2,3,100,100)").bind(vt).execute(&owner).await.unwrap();ids.lock().unwrap().push(vt);
        sqlx::query("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,media_id,state,created_at,updated_at) VALUES($1,'all',decode(repeat('ff',32),'hex'),decode(repeat('fe',32),'hex'),'Private vtuber',repeat('f',32),1,1,1)").bind(vt).execute(&owner).await.unwrap();
        let hidden=contests::listing(&public,vt,"all",0).await.unwrap().unwrap();assert!(!hidden.visible);assert!(hidden.entries.is_empty());
        sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=253402300799 WHERE id=$1").bind(vt).execute(&owner).await.unwrap();
        let visible=contests::listing(&public,vt,"all",0).await.unwrap().unwrap();assert!(visible.visible&&visible.voting);assert_eq!(visible.entries.len(),1);assert!(visible.entries[0].score.is_none());
        sqlx::query("UPDATE submission_private.contest_campaigns SET enabled=false WHERE id=$1").bind(vt).execute(&owner).await.unwrap();assert!(contests::listing(&public,vt,"all",0).await.unwrap().is_none());assert!(contests::campaign(&public,vt).await.unwrap().is_none());
    }}).await;
    let saved = ids.lock().unwrap().clone();
    for id in saved {
        for query in [
            "DELETE FROM submission_private.contest_votes WHERE campaign_id=$1",
            "DELETE FROM submission_private.contest_entries WHERE campaign_id=$1",
            "DELETE FROM submission_private.contest_campaigns WHERE id=$1",
        ] {
            sqlx::query(query).bind(id).execute(&owner).await.unwrap();
        }
    }
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    public.close().await;
    owner.close().await;
    work.unwrap();
}

#[tokio::test]
async fn operator_schedules_overlap_only_across_distinct_boards_and_close_without_erasure() {
    use board_store::site_submissions::admin::{self, contests as operator};
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let work=tokio::spawn({let owner=owner.clone();let public=public.clone();let ids=ids.clone();async move {
        let raw=r#"{"version":1,"kind":"board_banner","boards":["g"],"submission_starts":100,"submission_ends":200,"voting_starts":300,"voting_ends":400,"capacity":100,"vote_capacity":1000}"#;
        let first=operator::read(raw.as_bytes()).unwrap();let other=operator::read(raw.replace("[\"g\"]","[\"a\"]").as_bytes()).unwrap();
        let mut denied=public.acquire().await.unwrap();assert!(matches!(operator::open(&mut denied,&first).await,Err(admin::Error::Role)));drop(denied);
        let mut left=owner.acquire().await.unwrap();let mut right=owner.acquire().await.unwrap();
        let id=operator::open(&mut left,&first).await.unwrap();ids.lock().unwrap().push(id);
        assert!(matches!(operator::open(&mut left,&first).await,Err(admin::Error::ContestOverlap)));
        let distinct=operator::open(&mut right,&other).await.unwrap();ids.lock().unwrap().push(distinct);
        let next=operator::read(raw.replace("\"submission_starts\":100","\"submission_starts\":400").replace("\"submission_ends\":200","\"submission_ends\":500").replace("\"voting_starts\":300","\"voting_starts\":600").replace("\"voting_ends\":400","\"voting_ends\":700").as_bytes()).unwrap();
        let (a,b)=tokio::join!(operator::open(&mut left,&next),operator::open(&mut right,&next));assert_ne!(a.is_ok(),b.is_ok());
        let accepted=match (a,b){(Ok(id),Err(admin::Error::ContestOverlap))|(Err(admin::Error::ContestOverlap),Ok(id))=>id,_=>panic!("Exactly one matching schedule must open")};ids.lock().unwrap().push(accepted);
        let entry:i64=sqlx::query_scalar("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,media_id,state,score,created_at,updated_at) VALUES($1,'g',decode(repeat('91',32),'hex'),decode(repeat('92',32),'hex'),'Owned',repeat('9',32),1,1,1,1) RETURNING id").bind(id).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.contest_votes(campaign_id,board,voter_hash,entry_id,created_at) VALUES($1,'g',decode(repeat('93',32),'hex'),$2,1)").bind(id).bind(entry).execute(&owner).await.unwrap();
        assert!(operator::close(&mut left,id).await.unwrap());assert!(!operator::close(&mut left,id).await.unwrap());assert!(contests::campaign(&public,id).await.unwrap().is_none());
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM submission_private.contest_entries WHERE campaign_id=$1),(SELECT count(*) FROM submission_private.contest_votes WHERE campaign_id=$1)").bind(id).fetch_one(&owner).await.unwrap();assert_eq!(counts,(1,1));
    }}).await;
    let saved = ids.lock().unwrap().clone();
    for id in saved {
        for query in [
            "DELETE FROM submission_private.contest_votes WHERE campaign_id=$1",
            "DELETE FROM submission_private.contest_entries WHERE campaign_id=$1",
            "DELETE FROM submission_private.contest_campaigns WHERE id=$1",
        ] {
            sqlx::query(query).bind(id).execute(&owner).await.unwrap();
        }
    }
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    public.close().await;
    owner.close().await;
    work.unwrap();
}
