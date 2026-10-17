#![cfg(feature = "database-tests")]
use chrono::{DateTime, Duration, Timelike, Utc};
use sqlx::{PgPool, Postgres, Transaction};

static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn owner() -> PgPool {
    PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap()
}

async fn record(
    tx: &mut Transaction<'_, Postgres>,
    actor: Option<&[u8]>,
    now: Option<DateTime<Utc>>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT admission.record_dmca_warning($1,$2)")
        .bind(actor)
        .bind(now)
        .fetch_one(&mut **tx)
        .await
}

#[tokio::test]
async fn private_warning_window_capacity_and_role_boundaries() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("DELETE FROM admission.dmca_warnings")
        .execute(&mut *tx)
        .await
        .unwrap();
    let now = Utc::now().with_nanosecond(0).unwrap();
    let actor = [11u8; 32];
    for age in [-1, 0, 86399, 86400, 86401, 604800] {
        let last = now - Duration::seconds(age);
        sqlx::query("INSERT INTO admission.dmca_warnings VALUES($1,$2) ON CONFLICT(actor_hash) DO UPDATE SET last_warning_at=EXCLUDED.last_warning_at")
            .bind(actor.as_slice()).bind(last).execute(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            record(&mut tx, Some(&actor), Some(now)).await.unwrap(),
            age <= 86400
        );
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        let saved: DateTime<Utc> = sqlx::query_scalar(
            "SELECT last_warning_at FROM admission.dmca_warnings WHERE actor_hash=$1",
        )
        .bind(actor.as_slice())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            saved,
            if age <= 86400 { last } else { now },
            "Repeats must not extend the window"
        );
    }
    sqlx::query("UPDATE admission.file_effect_capacity SET warning_limit=1 WHERE singleton")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT capacity_case")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = record(&mut tx, Some(&[12; 32]), Some(now))
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("53300")
    );
    sqlx::query("ROLLBACK TO SAVEPOINT capacity_case")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE admission.dmca_warnings SET last_warning_at=$1")
        .bind(now - Duration::days(7))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SAVEPOINT retention_edge")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(record(&mut tx, Some(&[12; 32]), Some(now)).await.is_err());
    sqlx::query("ROLLBACK TO SAVEPOINT retention_edge")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE admission.dmca_warnings SET last_warning_at=last_warning_at-interval '1 second'",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!record(&mut tx, Some(&[12; 32]), Some(now)).await.unwrap());
    assert!(record(&mut tx, Some(&[12; 32]), Some(now)).await.unwrap());
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM admission.dmca_warnings")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 1);
    for (actor, clock) in [
        (None, Some(now)),
        (Some(&[1u8; 31][..]), Some(now)),
        (Some(&[1u8; 33][..]), Some(now)),
        (Some(&[1u8; 32][..]), None),
        (Some(&[1u8; 32][..]), Some(now - Duration::seconds(60))),
        (Some(&[1u8; 32][..]), Some(now + Duration::seconds(60))),
    ] {
        sqlx::query("SAVEPOINT invalid_case")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = record(&mut tx, actor, clock).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514")
        );
        sqlx::query("ROLLBACK TO SAVEPOINT invalid_case")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("SAVEPOINT missing_policy")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM admission.file_effect_capacity")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = record(&mut tx, Some(&[13; 32]), Some(now))
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("55000")
    );
    sqlx::query("ROLLBACK TO SAVEPOINT missing_policy")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE admission.file_effect_capacity SET warning_limit=1000 WHERE singleton")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.dmca_warnings SELECT sha256(convert_to('owned-expired-warning-'||n::text,'UTF8')),$1 FROM generate_series(1,257) n")
        .bind(now-Duration::days(8)).execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(!record(&mut tx, Some(&[14; 32]), Some(now)).await.unwrap());
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    let old: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admission.dmca_warnings WHERE last_warning_at<$1")
            .bind(now - Duration::days(7))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(old, 1, "One issuance may prune at most 256 expired entries");
    tx.rollback().await.unwrap();

    for name in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(name).unwrap())
            .await
            .unwrap();
        for query in [
            "SELECT * FROM admission.dmca_warnings",
            "SELECT * FROM admission.file_effect_capacity",
            "SELECT admission.record_dmca_warning(decode(repeat('11',32),'hex'),clock_timestamp())",
        ] {
            let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("42501")
            );
        }
        runtime.close().await;
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = record(&mut tx, Some(&actor), Some(Utc::now()))
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("55000")
    );
    tx.rollback().await.unwrap();
    owner.close().await;
}

#[tokio::test]
async fn simultaneous_first_attempts_serialize_without_extending_the_warning() {
    let _serial = TEST.lock().await;
    let owner = owner().await;
    let actor = board_domain::anonymous_session::Capability::generate()
        .unwrap()
        .storage_hash();
    let now = Utc::now().with_nanosecond(0).unwrap();
    let mut first = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *first)
        .await
        .unwrap();
    assert!(!record(&mut first, Some(&actor), Some(now)).await.unwrap());
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first)
        .await
        .unwrap();
    let another = owner.clone();
    let second = tokio::spawn(async move {
        let mut tx = another.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let repeated = record(&mut tx, Some(&actor), Some(now + Duration::seconds(1)))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        repeated
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
        )
        .bind(blocker)
        .fetch_one(&owner)
        .await
        .unwrap();
        if waiting {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Second warning did not reach the held actor lock"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!second.is_finished());
    first.commit().await.unwrap();
    assert!(second.await.unwrap());
    let saved: DateTime<Utc> = sqlx::query_scalar(
        "SELECT last_warning_at FROM admission.dmca_warnings WHERE actor_hash=$1",
    )
    .bind(actor.as_slice())
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(saved, now);
    sqlx::query("DELETE FROM admission.dmca_warnings WHERE actor_hash=$1")
        .bind(actor.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
}
