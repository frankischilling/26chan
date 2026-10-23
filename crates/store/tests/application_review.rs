#![cfg(feature = "database-tests")]
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};

struct Reviewer {
    account: i64,
    session: Vec<u8>,
    csrf: Vec<u8>,
}
struct Fixture {
    owner: PgPool,
    auth: PgPool,
    public: PgPool,
    staff: PgPool,
    campaign: i64,
    reviewer: Reviewer,
    administrator: Reviewer,
    ids: Vec<i64>,
}
async fn pool(name: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var(name).unwrap())
        .await
        .unwrap()
}
async fn reviewer(owner: &PgPool, role: &str) -> Reviewer {
    let account =
        sqlx::query_scalar("INSERT INTO staff_identity.accounts(role) VALUES($1) RETURNING id")
            .bind(role)
            .fetch_one(owner)
            .await
            .unwrap();
    let (credential,session,csrf):(Vec<u8>,Vec<u8>,Vec<u8>)=sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8'))").fetch_one(owner).await.unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)").bind(&session).bind(&csrf).bind(account).bind(credential).execute(owner).await.unwrap();
    Reviewer {
        account,
        session,
        csrf,
    }
}
fn code(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(expected),
        "{error}"
    );
}
impl Fixture {
    async fn page(&self, who: &Reviewer, state: i32, before: i64) -> Result<Value, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.application_review_page($1,$2,900,$3,$4,$5)")
            .bind(&who.session)
            .bind(&who.csrf)
            .bind(self.campaign)
            .bind(state)
            .bind(before)
            .fetch_one(&self.auth)
            .await
    }
    async fn detail(&self, who: &Reviewer, id: i64) -> Result<Option<Value>, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.application_review_detail($1,$2,900,$3)")
            .bind(&who.session)
            .bind(&who.csrf)
            .bind(id)
            .fetch_one(&self.auth)
            .await
    }
    async fn identity(&self, who: &Reviewer, id: i64) -> Result<Option<Value>, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.application_selected_identity($1,$2,900,$3)")
            .bind(&who.session)
            .bind(&who.csrf)
            .bind(id)
            .fetch_one(&self.auth)
            .await
    }
    async fn action(
        &self,
        who: &Reviewer,
        id: i64,
        revision: i32,
        action: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT submission_api.application_review_action($1,$2,900,$3,$4,$5)")
            .bind(&who.session)
            .bind(&who.csrf)
            .bind(id)
            .bind(revision)
            .bind(action)
            .fetch_one(&self.auth)
            .await
    }
}
#[tokio::test]
async fn application_review_hides_identity_and_serializes_revision_bound_selection() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let auth = pool("AUTH_DATABASE_URL").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL").await;
    let staff = pool("STAFF_DATABASE_URL").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let campaign: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO submission_private.application_campaigns(id,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity) VALUES($1,1,253402300799,ARRAY['g'],ARRAY['One','Two','Three','Four'],150,2000,100)").bind(campaign).execute(&owner).await.unwrap();
    let reviewer = reviewer(&owner, "moderator").await;
    let administrator = crate::reviewer(&owner, "admin").await;
    let mut ids = Vec::new();
    for n in 0..27 {
        let id:i64=sqlx::query_scalar("INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at) VALUES($1,21,-4,3,'Evenings','g',ARRAY['Owned <answer>','Two','Three','Four'],1,1) RETURNING id").bind(campaign).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.application_identity(application_id,campaign_id,ownership_hash,actor_hash,firstname,handle,email,email_identity) VALUES($1,$2,sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),'Private <name>','Private handle',$3,$3)").bind(id).bind(campaign).bind(format!("owned-{n}@example.invalid")).execute(&owner).await.unwrap();
        ids.push(id);
    }
    let accounts = [reviewer.account, administrator.account];
    let f = Fixture {
        owner: owner.clone(),
        auth: auth.clone(),
        public: public.clone(),
        staff: staff.clone(),
        campaign,
        reviewer,
        administrator,
        ids,
    };
    let result = tokio::spawn(exercise(f)).await;
    for query in [
        "DELETE FROM submission_private.application_review_log WHERE application_id IN(SELECT id FROM submission_private.applications WHERE campaign_id=$1)",
        "DELETE FROM submission_private.application_identity WHERE campaign_id=$1",
        "DELETE FROM submission_private.applications WHERE campaign_id=$1",
        "DELETE FROM submission_private.application_campaigns WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(campaign)
            .execute(&owner)
            .await
            .unwrap();
    }
    for account in accounts {
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
            sqlx::query("SELECT submission_api.application_review_page($1,$2,900,$3,0,0)")
                .bind(&f.reviewer.session)
                .bind(&f.reviewer.csrf)
                .bind(f.campaign)
                .execute(caller)
                .await
                .unwrap_err(),
            "42501",
        );
    }
    code(f.page(&f.reviewer, 0, 0).await.unwrap_err(), "P225A");
    sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['application_review'] WHERE id=$1")
        .bind(f.reviewer.account)
        .execute(&f.owner)
        .await
        .unwrap();
    let page = f.page(&f.reviewer, 0, 0).await.unwrap();
    assert_eq!(page["rows"].as_array().unwrap().len(), 25);
    assert_eq!(page["is_admin"], false);
    let tail = f
        .page(&f.reviewer, 0, page["next"].as_i64().unwrap())
        .await
        .unwrap();
    assert_eq!(tail["rows"].as_array().unwrap().len(), 2);
    assert_eq!(tail["next"], 0);
    let detail = f.detail(&f.reviewer, f.ids[0]).await.unwrap().unwrap();
    assert_eq!(detail["answers"][0], "Owned <answer>");
    assert_eq!(detail["questions"][0], "One");
    for value in [&page, &detail] {
        let text = value.to_string();
        for private in [
            "firstname",
            "handle",
            "email",
            "ownership_hash",
            "actor_hash",
            "Private",
            "example.invalid",
        ] {
            assert!(
                !text.contains(private),
                "{private} leaked into blind review"
            );
        }
    }
    code(
        f.identity(&f.reviewer, f.ids[0]).await.unwrap_err(),
        "P225A",
    );
    code(
        f.action(&f.reviewer, f.ids[0], 1, "accept")
            .await
            .unwrap_err(),
        "P225A",
    );
    assert!(
        f.identity(&f.administrator, f.ids[0])
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !f.action(&f.administrator, f.ids[0], 2, "accept")
            .await
            .unwrap()
    );
    sqlx::query("UPDATE submission_private.applications SET revision=2 WHERE id=$1")
        .bind(f.ids[0])
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(
        !f.action(&f.administrator, f.ids[0], 1, "accept")
            .await
            .unwrap()
    );
    assert!(
        f.action(&f.administrator, f.ids[0], 2, "accept")
            .await
            .unwrap()
    );
    assert!(
        !f.action(&f.administrator, f.ids[0], 2, "decline")
            .await
            .unwrap()
    );
    let identity = f
        .identity(&f.administrator, f.ids[0])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(identity["firstname"], "Private <name>");
    assert_eq!(identity.as_object().unwrap().len(), 4);
    assert_eq!(
        f.detail(&f.reviewer, f.ids[0]).await.unwrap().unwrap()["state"],
        2
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM staff_identity.credentials WHERE account_id=ANY($1)",
    )
    .bind(vec![f.reviewer.account, f.administrator.account])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(count, 2, "selection must not create staff credentials");
    // Distinct administrators cannot both transition the same application.
    sqlx::query("UPDATE staff_identity.accounts SET role='admin' WHERE id=$1")
        .bind(f.reviewer.account)
        .execute(&f.owner)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        f.action(&f.administrator, f.ids[1], 1, "accept"),
        f.action(&f.reviewer, f.ids[1], 1, "decline")
    );
    assert_ne!(a.unwrap(), b.unwrap());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submission_private.application_review_log WHERE application_id=$1",
    )
    .bind(f.ids[1])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(count, 1);
    // A concurrently committed applicant revision must invalidate the decision.
    let mut edit = f.owner.begin().await.unwrap();
    sqlx::query("UPDATE submission_private.applications SET revision=2 WHERE id=$1")
        .bind(f.ids[3])
        .execute(&mut *edit)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(f.action(&f.administrator, f.ids[3], 1, "accept"), async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        edit.commit().await.unwrap();
    });
    assert!(!result.unwrap());
    assert_eq!(
        f.detail(&f.reviewer, f.ids[3]).await.unwrap().unwrap()["state"],
        0
    );
    // A row wait cannot outlive recent authentication and still select.
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '599 seconds' WHERE token_hash=$1").bind(&f.administrator.session).execute(&f.owner).await.unwrap();
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM submission_private.applications WHERE id=$1 FOR UPDATE")
        .bind(f.ids[2])
        .execute(&mut *held)
        .await
        .unwrap();
    let (result, ()) = tokio::join!(f.action(&f.administrator, f.ids[2], 1, "accept"), async {
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        held.commit().await.unwrap();
    });
    code(result.unwrap_err(), "P225Q");
    let prior: i16 =
        sqlx::query_scalar("SELECT state FROM submission_private.applications WHERE id=$1")
            .bind(f.ids[2])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(prior, 0);
    let bad = Reviewer {
        account: f.reviewer.account,
        session: f.reviewer.session.clone(),
        csrf: vec![0; 32],
    };
    code(f.page(&bad, 0, 0).await.unwrap_err(), "P225A");
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
    sqlx::query(
        "GRANT SELECT ON submission_private.application_identity TO board_submission_review_owner",
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
