#![cfg(feature = "database-tests")]
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
struct Fixture {
    owner: PgPool,
    auth: PgPool,
    public: PgPool,
    staff: PgPool,
    profile: i64,
    account: i64,
    session: Vec<u8>,
    csrf: Vec<u8>,
    other_session: Vec<u8>,
    other_csrf: Vec<u8>,
    ids: Vec<i64>,
}
impl Fixture {
    async fn page(
        &self,
        state: i32,
        category: Option<i32>,
        before: i64,
    ) -> Result<Value, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.feedback_review_page($1,$2,900,$3,$4,$5,$6)")
            .bind(&self.session)
            .bind(&self.csrf)
            .bind(self.profile)
            .bind(state)
            .bind(category)
            .bind(before)
            .fetch_one(&self.auth)
            .await
    }
    async fn action(&self, id: i64, action: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.feedback_review_action($1,$2,900,$3,$4)")
            .bind(&self.session)
            .bind(&self.csrf)
            .bind(id)
            .bind(action)
            .fetch_one(&self.auth)
            .await
    }
    async fn role(&self, role: &str, flags: &[&str]) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$1,flags=$2 WHERE id=$3")
            .bind(role)
            .bind(flags)
            .bind(self.account)
            .execute(&self.owner)
            .await
            .unwrap();
    }
}
#[track_caller]
fn code(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(expected),
        "{error}"
    );
}
async fn pool(name: &str) -> PgPool {
    let mut options = PgPoolOptions::new().max_connections(5);
    if name == "AUTH_DATABASE_URL" {
        // Match the real staff role even in a minimal owned test cluster.
        options = options.after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='2s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        });
    }
    options
        .connect(&std::env::var(name).unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn feedback_review_is_role_bound_paged_atomic_and_expiry_checked() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let auth = pool("AUTH_DATABASE_URL").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL").await;
    let staff = pool("STAFF_DATABASE_URL").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let profile: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO submission_private.feedback_profiles(id,subject_limit,message_limit,capacity) VALUES($1,150,2000,100)").bind(profile).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO submission_private.feedback_categories VALUES($1,1,0,'First','One'),($1,2,1,'Second','Two')").bind(profile).execute(&owner).await.unwrap();
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    let credential: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&owner)
    .await
    .unwrap();
    let (session,csrf):(Vec<u8>,Vec<u8>)=sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8'))").fetch_one(&owner).await.unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)").bind(&session).bind(&csrf).bind(account).bind(&credential).execute(&owner).await.unwrap();
    let other_account: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES('admin') RETURNING id",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    let (other_credential,other_session,other_csrf):(Vec<u8>,Vec<u8>,Vec<u8>)=sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8'))").fetch_one(&owner).await.unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&other_credential)
    .bind(other_account)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)").bind(&other_session).bind(&other_csrf).bind(other_account).bind(&other_credential).execute(&owner).await.unwrap();
    let mut ids = Vec::new();
    for n in 0..29 {
        let id:i64=sqlx::query_scalar("INSERT INTO submission_private.feedback(profile_id,category,subject,message,state,actor_hash,peer,user_agent,language,created_at,updated_at,reply,reply_capcode) VALUES($1,$2,'Owned <subject>','Literal <body>',$3,decode(repeat('ab',32),'hex'),'192.0.2.225','Owned <agent>','en',1,1,$4,$5) RETURNING id")
            .bind(profile).bind(if n%2==0{1}else{2}).bind(if n<27{0i16}else{3i16}).bind((n>=27).then_some("Owned draft")).bind((n>=27).then_some("mod")).fetch_one(&owner).await.unwrap();
        ids.push(id);
    }
    let f = Fixture {
        owner: owner.clone(),
        auth: auth.clone(),
        public: public.clone(),
        staff: staff.clone(),
        profile,
        account,
        session,
        csrf,
        other_session,
        other_csrf,
        ids,
    };
    let result = tokio::spawn(exercise(f)).await;
    sqlx::query("DELETE FROM submission_private.feedback_review_log WHERE feedback_id IN(SELECT id FROM submission_private.feedback WHERE profile_id=$1)").bind(profile).execute(&owner).await.unwrap();
    for query in [
        "DELETE FROM submission_private.feedback WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(profile)
            .execute(&owner)
            .await
            .unwrap();
    }
    for account in [account, other_account] {
        for query in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(query)
                .bind(account)
                .execute(&owner)
                .await
                .unwrap();
        }
    }
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    for p in [owner, auth, public, staff] {
        p.close().await;
    }
    result.unwrap();
}
async fn exercise(f: Fixture) {
    for caller in [&f.public, &f.staff] {
        code(
            sqlx::query("SELECT submission_api.feedback_review_page($1,$2,900,$3,0,NULL,0)")
                .bind(&f.session)
                .bind(&f.csrf)
                .bind(f.profile)
                .execute(caller)
                .await
                .unwrap_err(),
            "42501",
        );
    }
    code(f.page(0, None, 0).await.unwrap_err(), "P225A");
    f.role("moderator", &["feedback_review"]).await;
    let page = f.page(0, None, 0).await.unwrap();
    assert_eq!(page["rows"].as_array().unwrap().len(), 25);
    assert_eq!(page["draft_count"], 2);
    assert_eq!(page["can_dismiss"], false);
    assert_eq!(page["is_admin"], false);
    let cursor = page["next"].as_i64().unwrap();
    assert!(cursor > 0);
    let tail = f.page(0, None, cursor).await.unwrap();
    assert_eq!(tail["rows"].as_array().unwrap().len(), 2);
    assert_eq!(tail["next"], 0);
    assert_eq!(
        f.page(0, Some(2), 0).await.unwrap()["rows"]
            .as_array()
            .unwrap()
            .len(),
        13
    );
    code(f.action(f.ids[0], "dismiss").await.unwrap_err(), "P225A");
    let detail: Value =
        sqlx::query_scalar("SELECT submission_api.feedback_review_detail($1,$2,900,$3)")
            .bind(&f.session)
            .bind(&f.csrf)
            .bind(f.ids[0])
            .fetch_one(&f.auth)
            .await
            .unwrap();
    assert_eq!(detail["message"], "Literal <body>");
    assert_eq!(detail["address"], "192.0.2.225");
    assert!(!detail.as_object().unwrap().contains_key("membership_hash"));
    code(
        sqlx::query("SELECT submission_api.feedback_review_detail($1,$2,900,$3)")
            .bind(&f.session)
            .bind([0u8; 32].as_slice())
            .bind(f.ids[0])
            .execute(&f.auth)
            .await
            .unwrap_err(),
        "P225A",
    );
    f.role("moderator", &["feedback_review", "feedback_dismiss"])
        .await;
    assert!(f.action(f.ids[0], "dismiss").await.unwrap());
    assert!(!f.action(f.ids[0], "dismiss").await.unwrap());
    code(f.action(f.ids[27], "approve").await.unwrap_err(), "P225A");
    f.role("admin", &[]).await;
    let (left, right) = tokio::join!(f.action(f.ids[2], "dismiss"), f.action(f.ids[2], "dismiss"));
    assert_ne!(left.unwrap(), right.unwrap());
    let other = sqlx::query_scalar::<_, bool>(
        "SELECT submission_api.feedback_review_action($1,$2,900,$3,'dismiss')",
    )
    .bind(&f.other_session)
    .bind(&f.other_csrf)
    .bind(f.ids[3])
    .fetch_one(&f.auth);
    let (left, right) = tokio::join!(f.action(f.ids[3], "dismiss"), other);
    assert_ne!(left.unwrap(), right.unwrap());

    assert!(f.action(f.ids[27], "approve").await.unwrap());
    assert!(!f.action(f.ids[27], "approve").await.unwrap());
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1").bind(&f.session).execute(&f.owner).await.unwrap();
    assert!(f.page(0, None, 0).await.is_ok());
    code(f.action(f.ids[1], "dismiss").await.unwrap_err(), "P225Q");
    sqlx::query(
        "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp() WHERE token_hash=$1",
    )
    .bind(&f.session)
    .execute(&f.owner)
    .await
    .unwrap();
    // A review that wakes after its session expires must leave the row and audit untouched.
    let expiry: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1 RETURNING expires_at")
        .bind(&f.session).fetch_one(&f.owner).await.unwrap();
    let mut gate = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM submission_private.feedback WHERE id=$1 FOR UPDATE")
        .bind(f.ids[1])
        .execute(&mut *gate)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    let release = async {
        tokio::time::timeout(std::time::Duration::from_secs(1),async {loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_auth' AND $1=ANY(pg_blocking_pids(pid)))").bind(blocker).fetch_one(&f.owner).await.unwrap();
            if waiting{break;}tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }}).await.unwrap();
        // Cross the database expiry boundary before releasing the row, without
        // overrunning the auth role's unchanged two-second lock deadline.
        tokio::time::timeout(std::time::Duration::from_millis(1500), async {
            loop {
                let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1")
                    .bind(expiry)
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
                if expired {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        gate.rollback().await.unwrap();
    };
    let (expired, ()) = tokio::join!(f.action(f.ids[1], "dismiss"), release);
    code(expired.unwrap_err(), "P225A");
    let state: i16 =
        sqlx::query_scalar("SELECT state FROM submission_private.feedback WHERE id=$1")
            .bind(f.ids[1])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(state, 0);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submission_private.feedback_review_log WHERE feedback_id=ANY($1)",
    )
    .bind(&f.ids)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(count, 4);
    let mut drift = f.owner.begin().await.unwrap();
    sqlx::query("GRANT SELECT(email) ON submission_private.application_identity TO board_submission_review_owner").execute(&mut *drift).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT submission_api.ready()")
            .fetch_one(&mut *drift)
            .await
            .unwrap()
    );
    drift.rollback().await.unwrap();
    let mut denied = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_review_owner")
        .execute(&mut *denied)
        .await
        .unwrap();
    code(
        sqlx::query("SELECT email FROM submission_private.application_identity LIMIT 0")
            .execute(&mut *denied)
            .await
            .unwrap_err(),
        "42501",
    );
    denied.rollback().await.unwrap();
    let mut drift = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_staff_post_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    sqlx::query(
        "ALTER FUNCTION staff_identity.lock_session(bytea,integer) SET search_path=pg_catalog",
    )
    .execute(&mut *drift)
    .await
    .unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT submission_api.ready()")
            .fetch_one(&mut *drift)
            .await
            .unwrap()
    );
    drift.rollback().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT submission_api.ready()")
            .fetch_one(&f.public)
            .await
            .unwrap()
    );
}
