#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use sqlx::{PgPool, Postgres, Transaction};
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Fixture {
    owner: PgPool,
    staff: PgPool,
    account: i64,
    session: [u8; 32],
    csrf: [u8; 32],
}
impl Fixture {
    async fn new() -> Self {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let account=sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('janitor',ARRAY['all'],'{}') RETURNING id").fetch_one(&owner).await.unwrap();
        let session = Capability::generate().unwrap().storage_hash();
        let csrf = Capability::generate().unwrap().storage_hash();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(session.as_slice())
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$1)").bind(session.as_slice()).bind(csrf.as_slice()).bind(account).execute(&owner).await.unwrap();
        Self {
            owner,
            staff,
            account,
            session,
            csrf,
        }
    }
    async fn guard(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        csrf: &[u8],
        board: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT content.revalidate_staff_refusal($1,$2,3600,$3)")
            .bind(self.session.as_slice())
            .bind(csrf)
            .bind(board)
            .execute(&mut **tx)
            .await
            .map(|_| ())
    }
    async fn cleanup(self) {
        for sql in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(sql)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        self.staff.close().await;
        self.owner.close().await;
    }
}
fn code(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some(expected)
    );
}
#[tokio::test]
async fn refusal_guard_checks_all_staff_ranks_and_live_scope_without_activity() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let before: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM staff_identity.sessions s WHERE token_hash=$1")
            .bind(f.session.as_slice())
            .fetch_one(&f.owner)
            .await
            .unwrap();
    for role in ["janitor", "moderator", "manager", "admin"] {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,flags='{}' WHERE id=$1")
            .bind(f.account)
            .bind(role)
            .execute(&f.owner)
            .await
            .unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        f.guard(&mut tx, &f.csrf, "a").await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        code(f.guard(&mut tx, &[0; 32], "a").await.unwrap_err(), "28000");
        tx.rollback().await.unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        code(
            f.guard(&mut tx, &f.csrf, "test").await.unwrap_err(),
            "28000",
        );
        tx.rollback().await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['developer'] WHERE id=$1")
            .bind(f.account)
            .execute(&f.owner)
            .await
            .unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        let result = f.guard(&mut tx, &f.csrf, "test").await;
        if role == "janitor" {
            code(result.unwrap_err(), "28000");
        } else {
            result.unwrap();
        }
        tx.rollback().await.unwrap();
    }
    let after: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM staff_identity.sessions s WHERE token_hash=$1")
            .bind(f.session.as_slice())
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(before, after);
    for sql in [
        "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
        "UPDATE staff_identity.accounts SET deny_boards=ARRAY['a'] WHERE id=$1",
    ] {
        sqlx::query(sql)
            .bind(f.account)
            .execute(&f.owner)
            .await
            .unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        code(f.guard(&mut tx, &f.csrf, "a").await.unwrap_err(), "28000");
        tx.rollback().await.unwrap();
        sqlx::query(
            "UPDATE staff_identity.accounts SET revoked_at=NULL,deny_boards='{}' WHERE id=$1",
        )
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
    }
    for sql in [
        "UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1",
        "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1",
        "UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp()-interval '61 minutes' WHERE token_hash=$1",
    ] {
        sqlx::query(sql)
            .bind(f.session.as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        let mut tx = f.staff.begin().await.unwrap();
        code(f.guard(&mut tx, &f.csrf, "a").await.unwrap_err(), "28000");
        tx.rollback().await.unwrap();
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour',authenticated_at=clock_timestamp(),last_activity_at=clock_timestamp() WHERE token_hash=$1").bind(f.session.as_slice()).execute(&f.owner).await.unwrap();
    }
    f.cleanup().await;
}
#[tokio::test]
async fn refusal_guard_holds_authority_until_commit_and_denies_other_runtime_roles() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let mut held = f.staff.begin().await.unwrap();
    f.guard(&mut held, &f.csrf, "a").await.unwrap();
    let mut observer = f.owner.begin().await.unwrap();
    for sql in [
        "SELECT token_hash FROM staff_identity.sessions WHERE token_hash=$1 FOR UPDATE NOWAIT",
        "SELECT id FROM staff_identity.credentials WHERE id=$1 FOR UPDATE NOWAIT",
        "SELECT a.id FROM staff_identity.accounts a JOIN staff_identity.sessions s ON s.account_id=a.id WHERE s.token_hash=$1 FOR UPDATE OF a NOWAIT",
    ] {
        sqlx::query("SAVEPOINT authority_lock")
            .execute(&mut *observer)
            .await
            .unwrap();
        code(
            sqlx::query(sql)
                .bind(f.session.as_slice())
                .execute(&mut *observer)
                .await
                .unwrap_err(),
            "55P03",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT authority_lock")
            .execute(&mut *observer)
            .await
            .unwrap();
    }
    sqlx::query("SET LOCAL lock_timeout='500ms'")
        .execute(&mut *observer)
        .await
        .unwrap();
    sqlx::query("UPDATE staff_identity.credentials SET credential=credential WHERE id=$1")
        .bind(f.session.as_slice())
        .execute(&mut *observer)
        .await
        .unwrap();
    observer.rollback().await.unwrap();
    held.commit().await.unwrap();
    for name in ["TEST_PUBLIC_DATABASE_URL", "AUTH_DATABASE_URL"] {
        let pool = PgPool::connect(&std::env::var(name).unwrap())
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        code(f.guard(&mut tx, &f.csrf, "a").await.unwrap_err(), "42501");
        tx.rollback().await.unwrap();
        pool.close().await;
    }
    let mut owner = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE")
        .bind(f.account)
        .execute(&mut *owner)
        .await
        .unwrap();
    let mut tx = f.staff.begin().await.unwrap();
    code(f.guard(&mut tx, &f.csrf, "a").await.unwrap_err(), "28000");
    tx.rollback().await.unwrap();
    owner.rollback().await.unwrap();
    f.cleanup().await;
}
