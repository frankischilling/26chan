#![cfg(feature = "database-tests")]

use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;

// Capacity fixtures are transaction-local and must not overlap other tests here.
static QUOTA_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pools() -> (PgPool, PgPool) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    (owner, public)
}

async fn actor(owner: &PgPool) -> Vec<u8> {
    sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
        .fetch_one(owner)
        .await
        .unwrap()
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

async fn reserve(tx: &mut Transaction<'_, Postgres>, actor: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT content.reserve_public_deletion($1)")
        .bind(actor)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

// Fixture mutations stay in the migrator transaction. Exercise the definer
// through its existing SET-only membership without granting new privileges.
// The savepoint also restores the role after an expected SQL exception.
async fn reserve_as_owner(
    tx: &mut Transaction<'_, Postgres>,
    actor: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("SAVEPOINT quota_owner_reservation")
        .execute(&mut **tx)
        .await?;
    sqlx::query("SET LOCAL ROLE board_public_deletion_owner")
        .execute(&mut **tx)
        .await?;
    let result = reserve(tx, actor).await;
    if result.is_err() {
        sqlx::query("ROLLBACK TO SAVEPOINT quota_owner_reservation")
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("RESET ROLE").execute(&mut **tx).await?;
    sqlx::query("RELEASE SAVEPOINT quota_owner_reservation")
        .execute(&mut **tx)
        .await?;
    result
}

async fn now(tx: &mut Transaction<'_, Postgres>) -> i64 {
    sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
        .fetch_one(&mut **tx)
        .await
        .unwrap()
}

async fn cleanup(owner: &PgPool, actor: &[u8]) {
    sqlx::query("DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
        .bind(actor)
        .execute(owner)
        .await
        .unwrap();
}

#[tokio::test]
async fn three_successes_then_hourly_flood_and_reservation_rolls_back() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, public) = pools().await;
    let key = actor(&owner).await;
    // A rejected downstream mutation drops its reservation, including creation.
    let mut tx = public.begin().await.unwrap();
    reserve(&mut tx, &key).await.unwrap();
    tx.rollback().await.unwrap();
    let absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.public_deletion_actors WHERE actor_hash=$1)",
    )
    .bind(&key)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert!(absent);
    for expected in 1..=3 {
        let mut tx = public.begin().await.unwrap();
        reserve(&mut tx, &key).await.unwrap();
        tx.commit().await.unwrap();
        let count: i32 = sqlx::query_scalar(
            "SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
        )
        .bind(&key)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!(count, expected);
        if expected == 1 {
            let mut rejected = public.begin().await.unwrap();
            reserve(&mut rejected, &key).await.unwrap();
            // Model failure later in the mutation transaction, not just an explicit rollback.
            assert!(
                sqlx::query("SELECT 1/0")
                    .execute(&mut *rejected)
                    .await
                    .is_err()
            );
            rejected.rollback().await.unwrap();
        }
    }
    let mut tx = public.begin().await.unwrap();
    assert_eq!(code(&reserve(&mut tx, &key).await.unwrap_err()), "P0081");
    tx.rollback().await.unwrap();
    let state: (i32, bool) = sqlx::query_as(
        "SELECT cardinality(events),expires_at=(SELECT max(e)+86400 FROM unnest(events) e) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
    ).bind(&key).fetch_one(&owner).await.unwrap();
    assert_eq!(state, (3, true));
    cleanup(&owner, &key).await;
}

// Exercise exact second boundaries without sleeping or assuming that a Rust/SQL
// round trip cannot cross a clock tick. A crossed tick rolls back and retries the
// entire sample; all assertions use a verified unchanged server clock second.
async fn boundary_case(owner: &PgPool, offsets: &[i64], expected: Option<&str>, count: i32) {
    let key = actor(owner).await;
    for _ in 0..32 {
        let mut tx = owner.begin().await.unwrap();
        let before = now(&mut tx).await;
        let events: Vec<i64> = offsets.iter().map(|offset| before + offset).collect();
        sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,$2,$3)")
            .bind(&key).bind(&events).bind(before + 86400).execute(&mut *tx).await.unwrap();
        sqlx::query("SAVEPOINT boundary_attempt")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = reserve_as_owner(&mut tx, &key).await;
        let actual = result.as_ref().err().map(code);
        if result.is_err() {
            sqlx::query("ROLLBACK TO SAVEPOINT boundary_attempt")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        let after = now(&mut tx).await;
        let retained: i32 = sqlx::query_scalar("SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
            .bind(&key).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        if before == after {
            assert_eq!(actual.as_deref(), expected, "offsets={offsets:?}");
            assert_eq!(retained, count, "offsets={offsets:?}");
            return;
        }
    }
    panic!("could not obtain a boundary sample within one server clock second");
}

#[tokio::test]
async fn inclusive_hour_and_day_edges_daily_threshold_and_hourly_precedence() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, _) = pools().await;
    boundary_case(&owner, &[-3600; 3], Some("P0081"), 3).await;
    boundary_case(&owner, &[-3601; 3], None, 4).await;
    boundary_case(&owner, &[-7200; 10], None, 11).await;
    boundary_case(&owner, &[-7200; 11], Some("P0082"), 11).await;
    boundary_case(&owner, &[-86400; 11], Some("P0082"), 11).await;
    let mut partly_expired = [-86400; 11];
    partly_expired[0] = -86401;
    boundary_case(&owner, &partly_expired, None, 11).await;
    boundary_case(&owner, &[-86401; 11], None, 1).await;
    let mut both = [-7200; 11];
    both[..3].fill(0);
    boundary_case(&owner, &both, Some("P0081"), 11).await;
}

#[tokio::test]
async fn concurrent_new_actor_creation_serializes_exactly_three_successes() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, public) = pools().await;
    let key = actor(&owner).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(12));
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let public = public.clone();
        let key = key.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut tx = public.begin().await.unwrap();
            match reserve(&mut tx, &key).await {
                Ok(()) => {
                    tx.commit().await.unwrap();
                    true
                }
                Err(error) => {
                    assert_eq!(code(&error), "P0081");
                    tx.rollback().await.unwrap();
                    false
                }
            }
        }));
    }
    let mut successes = 0;
    for task in tasks {
        successes += usize::from(task.await.unwrap());
    }
    assert_eq!(successes, 3);
    let count: i32 = sqlx::query_scalar(
        "SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
    )
    .bind(&key)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(count, 3);
    cleanup(&owner, &key).await;
}

#[tokio::test]
async fn malformed_hashes_arrays_and_runtime_acl_are_rejected() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, public) = pools().await;
    for value in [
        None,
        Some(vec![]),
        Some(vec![1_u8; 31]),
        Some(vec![1_u8; 33]),
    ] {
        for query in [
            "SELECT content.reserve_public_deletion($1)",
            "SELECT content.check_public_deletion_quota($1)",
        ] {
            let error = sqlx::query(query)
                .bind(value.as_deref())
                .execute(&public)
                .await
                .unwrap_err();
            assert_eq!(code(&error), "23514");
        }
    }
    for (table, read_query) in [
        (
            "public_deletion_actors",
            "SELECT * FROM post_secrets.public_deletion_actors",
        ),
        (
            "public_deletion_capacity",
            "SELECT * FROM post_secrets.public_deletion_capacity",
        ),
    ] {
        for role in [
            "board_public",
            "board_staff",
            "board_auth",
            "board_media",
            "board_media_read",
            "board_monitor",
            "board_media_intake",
        ] {
            let privilege: bool = sqlx::query_scalar("SELECT has_table_privilege($1,$2,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')")
                .bind(role).bind(format!("post_secrets.{table}")).fetch_one(&owner).await.unwrap();
            assert!(!privilege, "{role} has private access to {table}");
        }
        let error = sqlx::query(read_query)
            .fetch_all(&public)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "42501");
    }
    let locked_down: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_public_deletion_owner' AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND NOT has_schema_privilege(r.oid,'content','CREATE') AND NOT has_schema_privilege(r.oid,'post_secrets','CREATE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid='content.reserve_public_deletion(bytea)'::regprocedure")
        .fetch_one(&owner).await.unwrap();
    assert!(locked_down);
    let membership: bool = sqlx::query_scalar("SELECT m.set_option AND NOT m.inherit_option AND NOT m.admin_option FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member WHERE r.rolname='board_public_deletion_owner' AND u.rolname='board_migrator'")
        .fetch_one(&owner).await.unwrap();
    assert!(membership);
    let executable: bool = sqlx::query_scalar("SELECT has_function_privilege('board_public','content.reserve_public_deletion(bytea)','EXECUTE') AND NOT EXISTS(SELECT 1 FROM pg_proc p, LATERAL aclexplode(p.proacl) a WHERE p.oid='content.reserve_public_deletion(bytea)'::regprocedure AND a.grantee=0 AND a.privilege_type='EXECUTE')")
        .fetch_one(&owner).await.unwrap();
    assert!(executable);
    for insert_query in [
        "INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,NULL::bigint[],1)",
        "INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[NULL]::bigint[],1)",
        "INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[-1]::bigint[],1)",
        "INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[[1,2],[3,4]]::bigint[],1)",
        "INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,array_fill(1::bigint,ARRAY[12]),1)",
    ] {
        let key = actor(&owner).await;
        let error = sqlx::query(insert_query)
            .bind(&key)
            .execute(&owner)
            .await
            .unwrap_err();
        assert!(["23502", "23514", "2202E"].contains(&code(&error).as_str()));
    }
}

#[tokio::test]
async fn capacity_never_evicts_live_actors_and_cleanup_is_bounded() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, _) = pools().await;
    let mut tx = owner.begin().await.unwrap();
    // A single rollback-only synthetic population exercises the real fixed cap.
    // Run this test binary in the isolated database lane, not against a service.
    sqlx::query("DELETE FROM post_secrets.public_deletion_actors")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) SELECT sha256(convert_to('quota-capacity-'||n::text,'UTF8')),ARRAY[floor(extract(epoch FROM clock_timestamp()))::bigint-7200],floor(extract(epoch FROM clock_timestamp()))::bigint+86400 FROM generate_series(1,100000) n")
        .execute(&mut *tx).await.unwrap();
    let existing: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to('quota-capacity-100000','UTF8'))")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    reserve_as_owner(&mut tx, &existing).await.unwrap();
    let key = vec![42_u8; 32];
    sqlx::query("SAVEPOINT full_capacity")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        code(&reserve_as_owner(&mut tx, &key).await.unwrap_err()),
        "P0083"
    );
    sqlx::query("ROLLBACK TO SAVEPOINT full_capacity")
        .execute(&mut *tx)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.public_deletion_actors")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 100000);
    sqlx::query("UPDATE post_secrets.public_deletion_actors SET events=ARRAY[0::bigint],expires_at=86400 WHERE actor_hash IN(SELECT sha256(convert_to('quota-capacity-'||n::text,'UTF8')) FROM generate_series(1,65) n)")
        .execute(&mut *tx).await.unwrap();
    reserve_as_owner(&mut tx, &key).await.unwrap();
    let counts: (i64, i64) = sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE expires_at=86400) FROM post_secrets.public_deletion_actors").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(counts, (99937, 1));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn cleanup_keeps_the_inclusive_day_boundary() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, _) = pools().await;
    let boundary = actor(&owner).await;
    let expired = actor(&owner).await;
    let fresh = actor(&owner).await;
    for _ in 0..32 {
        let mut tx = owner.begin().await.unwrap();
        // Isolate the bounded cleaner from unrelated older fixtures, all rollback-only.
        sqlx::query("DELETE FROM post_secrets.public_deletion_actors")
            .execute(&mut *tx)
            .await
            .unwrap();
        let before = now(&mut tx).await;
        sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[$3::bigint-86400],$3),($2,ARRAY[$3::bigint-86401],$3-1)")
            .bind(&boundary).bind(&expired).bind(before).execute(&mut *tx).await.unwrap();
        reserve_as_owner(&mut tx, &fresh).await.unwrap();
        let after = now(&mut tx).await;
        let remains: (bool, bool) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM post_secrets.public_deletion_actors WHERE actor_hash=$1),EXISTS(SELECT 1 FROM post_secrets.public_deletion_actors WHERE actor_hash=$2)")
            .bind(&boundary).bind(&expired).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        if before == after {
            assert_eq!(remains, (true, false));
            return;
        }
    }
    panic!("could not obtain a cleanup boundary sample within one server clock second");
}

#[tokio::test]
async fn advisory_check_reports_flood_without_mutating_or_creating_history() {
    let _guard = QUOTA_TEST.lock().await;
    let (owner, public) = pools().await;
    let private_definer: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_public_deletion_owner' AND has_function_privilege('board_public',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid='content.check_public_deletion_quota(bytea)'::regprocedure")
        .fetch_one(&owner).await.unwrap();
    assert!(private_definer);
    let missing = actor(&owner).await;
    sqlx::query("SELECT content.check_public_deletion_quota($1)")
        .bind(&missing)
        .execute(&public)
        .await
        .unwrap();
    let absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.public_deletion_actors WHERE actor_hash=$1)",
    )
    .bind(&missing)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert!(absent);
    for (count, age, expected) in [(3_i32, 0_i64, "P0081"), (11, 7200, "P0082")] {
        let key = actor(&owner).await;
        sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) SELECT $1,array_fill(floor(extract(epoch FROM clock_timestamp()))::bigint-$2,ARRAY[$3::integer]),floor(extract(epoch FROM clock_timestamp()))::bigint+86400")
            .bind(&key).bind(age).bind(count).execute(&owner).await.unwrap();
        let before: (Vec<i64>, i64) = sqlx::query_as(
            "SELECT events,expires_at FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
        )
        .bind(&key)
        .fetch_one(&owner)
        .await
        .unwrap();
        let error = sqlx::query("SELECT content.check_public_deletion_quota($1)")
            .bind(&key)
            .execute(&public)
            .await
            .unwrap_err();
        assert_eq!(code(&error), expected);
        let after: (Vec<i64>, i64) = sqlx::query_as(
            "SELECT events,expires_at FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
        )
        .bind(&key)
        .fetch_one(&owner)
        .await
        .unwrap();
        assert_eq!(before, after);
        cleanup(&owner, &key).await;
    }
}
