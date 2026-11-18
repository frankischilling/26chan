#![cfg(feature = "database-tests")]
use sqlx::{PgPool, postgres::PgPoolOptions};

async fn login(variable: &str, role: &str) -> PgPool {
    let owned_migrator = role == "board_migrator";
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .after_connect(move |connection, _| {
            Box::pin(async move {
                if owned_migrator {
                    sqlx::query("SET statement_timeout='10s'")
                        .execute(connection)
                        .await?;
                }
                Ok(())
            })
        })
        .connect(&std::env::var(variable).expect("owned test database URL"))
        .await
        .unwrap();
    let identities: (String, String) =
        sqlx::query_as("SELECT current_user::text,session_user::text")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(identities, (role.into(), role.into()));
    pool
}

#[tokio::test]
async fn runtime_profile_reads_do_not_expose_submission_identity_or_open_workflows() {
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let result = tokio::spawn(check(public.clone(), owner.clone(), id)).await;
    for query in [
        "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
        "DELETE FROM submission_private.application_campaigns WHERE id=$1",
        "DELETE FROM submission_private.contest_campaigns WHERE id=$1",
    ] {
        sqlx::query(query).bind(id).execute(&owner).await.unwrap();
    }
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    public.close().await;
    owner.close().await;
    result.unwrap();
}
async fn check(public: PgPool, owner: PgPool, id: i64) {
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT submission_api.ready()")
            .fetch_one(&public)
            .await
            .unwrap()
    );
    for query in [
        "SELECT submission_api.feedback_profile()",
        "SELECT submission_api.application_profile()",
        "SELECT submission_api.contest_profile(1::smallint)",
        "SELECT submission_api.contest_profile(2::smallint)",
    ] {
        assert!(
            sqlx::query_scalar::<_, Option<serde_json::Value>>(query)
                .fetch_one(&public)
                .await
                .unwrap()
                .is_none(),
            "Use an owned empty submission database"
        );
    }
    for query in [
        "SELECT * FROM submission_private.application_identity",
        "SELECT * FROM submission_private.applications",
        "SELECT * FROM submission_private.feedback",
        "SELECT * FROM submission_private.contest_entries",
        "SELECT * FROM submission_private.contest_votes",
        "SET ROLE board_submission_owner",
        "SET ROLE board_migrator",
    ] {
        let error = sqlx::query(query).execute(&public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    // The function owner itself cannot read a contact field or bearer digest.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("SELECT firstname FROM submission_private.application_identity")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("42501")
    );
    tx.rollback().await.unwrap();
    profiles(&public, &owner, id).await;
}

async fn profiles(public: &PgPool, owner: &PgPool, id: i64) {
    use board_domain::site_submissions::contests::{Kind, Phase};
    use board_store::site_submissions as store;
    assert!(store::ready(public).await.unwrap());
    assert!(store::feedback_profile(public).await.unwrap().is_none());
    assert!(store::application_profile(public).await.unwrap().is_none());
    assert!(
        store::contest_profile(public, Kind::BoardBanner)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("INSERT INTO submission_private.feedback_profiles(id,enabled,subject_limit,message_limit,capacity) VALUES($1,true,150,2000,10)")
        .bind(id).execute(owner).await.unwrap();
    assert!(
        store::feedback_profile(public).await.unwrap().is_none(),
        "No implicit category seeds"
    );
    sqlx::query("INSERT INTO submission_private.feedback_categories(profile_id,id,ordinal,label,short_label) VALUES($1,2,0,'Owned technical','Technical'),($1,1,1,'Owned general','General')")
        .bind(id).execute(owner).await.unwrap();
    let form = store::feedback_profile(public).await.unwrap().unwrap();
    assert_eq!(form.id, id);
    assert_eq!(form.form.categories[0].id, 2);
    assert_eq!(form.form.subject_limit, 150);
    sqlx::query("INSERT INTO submission_private.application_campaigns(id,enabled,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity) VALUES($1,true,1,2,ARRAY['g'],ARRAY['One','Two','Three','Four'],150,2000,10)")
        .bind(id).execute(owner).await.unwrap();
    assert!(
        store::application_profile(public).await.unwrap().is_none(),
        "Expired campaign stays closed"
    );
    sqlx::query("UPDATE submission_private.application_campaigns SET ends_at=floor(extract(epoch FROM clock_timestamp()))::bigint+3600 WHERE id=$1")
        .bind(id).execute(owner).await.unwrap();
    let form = store::application_profile(public).await.unwrap().unwrap();
    assert_eq!(form.id, id);
    assert_eq!(form.form.boards, ["g"]);
    assert_eq!(form.form.questions, ["One", "Two", "Three", "Four"]);
    sqlx::query("INSERT INTO submission_private.contest_campaigns(id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity) VALUES($1,1,true,ARRAY['g'],1,2,3,4,10,100)")
        .bind(id).execute(owner).await.unwrap();
    let form = store::contest_profile(public, Kind::BoardBanner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(form.campaign.id, id);
    assert_eq!(
        form.campaign.phase(form.current_time).unwrap(),
        Phase::Results
    );
    assert!(
        store::contest_profile(public, Kind::Vtuber)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("UPDATE submission_private.contest_campaigns SET enabled=false WHERE id=$1")
        .bind(id)
        .execute(owner)
        .await
        .unwrap();
    assert!(
        store::contest_profile(public, Kind::BoardBanner)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn operator_feedback_profiles_require_real_migrator_and_retain_history() {
    use board_store::site_submissions::{self as store, admin};
    use std::sync::{Arc, Mutex};
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let ids=ids.clone();async move {
        let profile=admin::read_feedback(br#"{"version":1,"subject_limit":150,"message_limit":2000,"capacity":100,"cooldown_seconds":60,"categories":[{"id":1,"label":"Owned","short_label":"Owned"}]}"#.as_slice()).unwrap();
        let mut denied=public.acquire().await.unwrap();
        assert!(matches!(admin::start_feedback(&mut denied,&profile).await,Err(admin::Error::Role)));
        drop(denied);
        let mut connection=owner.acquire().await.unwrap();
        let first=admin::start_feedback(&mut connection,&profile).await.unwrap();ids.lock().unwrap().push(first);
        assert_eq!(store::feedback_profile(&public).await.unwrap().unwrap().id,first);
        assert!(matches!(admin::start_feedback(&mut connection,&profile).await,Err(admin::Error::Active)));
        sqlx::query("INSERT INTO submission_private.feedback(profile_id,category,subject,message,actor_hash,peer,user_agent,language,created_at,updated_at) VALUES($1,1,'Owned','Retained',decode(repeat('ab',32),'hex'),'192.0.2.225','Owned','en',1,1)").bind(first).execute(&owner).await.unwrap();
        assert!(admin::close_feedback(&mut connection,first).await.unwrap());
        assert!(!admin::close_feedback(&mut connection,first).await.unwrap());
        assert!(store::feedback_profile(&public).await.unwrap().is_none());
        let retained:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.feedback WHERE profile_id=$1").bind(first).fetch_one(&owner).await.unwrap();assert_eq!(retained,1);
        drop(connection);
        let mut left=owner.acquire().await.unwrap();let mut right=owner.acquire().await.unwrap();
        let (a,b)=tokio::join!(admin::start_feedback(&mut left,&profile),admin::start_feedback(&mut right,&profile));
        assert_ne!(a.is_ok(),b.is_ok());
        let (next,refused)=match (a,b){(Ok(id),Err(error))|(Err(error),Ok(id))=>(id,error),_=>panic!("Exactly one operator start must win")};
        assert!(matches!(refused,admin::Error::Active));assert_ne!(next,first);ids.lock().unwrap().push(next);
        assert!(admin::close_feedback(&mut left,next).await.unwrap());
    }}).await;
    let saved = ids.lock().unwrap().clone();
    for id in saved {
        for query in [
            "DELETE FROM submission_private.feedback WHERE profile_id=$1",
            "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
            "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
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
    result.unwrap();
}

#[tokio::test]
async fn operator_application_campaigns_require_real_migrator_and_retain_history() {
    use board_store::site_submissions::{self as store, admin};
    use std::sync::{Arc, Mutex};
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let ids=ids.clone();async move {
        let profile=admin::applications::read(br#"{"version":1,"starts_at":1,"ends_at":253402300799,"boards":["g"],"questions":["One","Two","Three","Four"],"field_limit":150,"answer_limit":2000,"capacity":100,"cooldown_seconds":60}"#.as_slice()).unwrap();
        let mut denied=public.acquire().await.unwrap();
        assert!(matches!(admin::applications::open(&mut denied,&profile).await,Err(admin::Error::Role)));
        drop(denied);
        let mut connection=owner.acquire().await.unwrap();
        let first=admin::applications::open(&mut connection,&profile).await.unwrap();ids.lock().unwrap().push(first);
        assert_eq!(store::application_profile(&public).await.unwrap().unwrap().id,first);
        assert!(matches!(admin::applications::open(&mut connection,&profile).await,Err(admin::Error::ApplicationActive)));
        sqlx::query("INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at) VALUES($1,25,0,1,'Owned','g',ARRAY['one','two','three','four'],1,1)").bind(first).execute(&owner).await.unwrap();
        assert!(admin::applications::close(&mut connection,first).await.unwrap());
        assert!(!admin::applications::close(&mut connection,first).await.unwrap());
        assert!(store::application_profile(&public).await.unwrap().is_none());
        let retained:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1").bind(first).fetch_one(&owner).await.unwrap();assert_eq!(retained,1);
        drop(connection);
        let mut left=owner.acquire().await.unwrap();let mut right=owner.acquire().await.unwrap();
        let (a,b)=tokio::join!(admin::applications::open(&mut left,&profile),admin::applications::open(&mut right,&profile));
        assert_ne!(a.is_ok(),b.is_ok());
        let (next,refused)=match (a,b){(Ok(id),Err(error))|(Err(error),Ok(id))=>(id,error),_=>panic!("Exactly one operator start must win")};
        assert!(matches!(refused,admin::Error::ApplicationActive));assert_ne!(next,first);ids.lock().unwrap().push(next);
        assert!(admin::applications::close(&mut left,next).await.unwrap());
    }}).await;
    let saved = ids.lock().unwrap().clone();
    for id in saved {
        for query in [
            "DELETE FROM submission_private.applications WHERE campaign_id=$1",
            "DELETE FROM submission_private.application_campaigns WHERE id=$1",
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
    result.unwrap();
}
