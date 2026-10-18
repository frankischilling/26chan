#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn owner() -> PgPool {
    PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap()
}

async fn record(
    tx: &mut Transaction<'_, Postgres>,
    actor: &[u8],
    token: Option<&[u8]>,
    days: i32,
    dmca: bool,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT restrictions.record_file_ban($1,$2,'a',189,decode(repeat('42',16),'hex'),$3,$4,'Public reason','Private reason','UTC')")
        .bind(actor).bind(token).bind(days).bind(dmca).fetch_one(&mut **tx).await
}

#[tokio::test]
async fn private_expiry_matches_captured_source_dates_and_rejects_unknown_policy() {
    let _serial = TEST.lock().await;
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../domain/tests/fixtures/automatic-ban-issuer-reference.json"
    ))
    .unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 420);
    for case in cases {
        if case["exempt"].as_bool().unwrap() {
            continue;
        }
        let days = case["days"].as_i64().unwrap() as i32;
        sqlx::query("SAVEPOINT expiry_case")
            .execute(&mut *tx)
            .await
            .unwrap();
        let actual: Result<Option<DateTime<Utc>>, sqlx::Error> = sqlx::query_scalar(
            "SELECT restrictions.file_expiry(to_timestamp($1::bigint::double precision),$2,$3)",
        )
        .bind(case["clock"].as_i64().unwrap())
        .bind(days)
        .bind(case["timezone"].as_str().unwrap())
        .fetch_one(&mut *tx)
        .await;
        if days < -1 {
            assert_eq!(
                actual
                    .unwrap_err()
                    .as_database_error()
                    .unwrap()
                    .code()
                    .as_deref(),
                Some("23514")
            );
        } else {
            assert_eq!(
                actual.unwrap().map(|value| value.timestamp()),
                case["aligned_expiry"].as_i64(),
                "{case}"
            );
        }
        sqlx::query("ROLLBACK TO SAVEPOINT expiry_case")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for expression in [
        "SELECT restrictions.file_expiry(NULL,0,'UTC')",
        "SELECT restrictions.file_expiry('infinity',0,'UTC')",
        "SELECT restrictions.file_expiry('-infinity',0,'UTC')",
        "SELECT restrictions.file_expiry(now(),NULL,'UTC')",
        "SELECT restrictions.file_expiry(now(),3651,'UTC')",
        "SELECT restrictions.file_expiry(now(),0,NULL)",
        "SELECT restrictions.file_expiry(now(),0,'')",
        "SELECT restrictions.file_expiry(now(),0,'unrecognized/zone')",
        "SELECT restrictions.file_expiry(now(),0,'UTC+13')",
    ] {
        sqlx::query("SAVEPOINT invalid_expiry")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(expression).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514"),
            "{expression}"
        );
        sqlx::query("ROLLBACK TO SAVEPOINT invalid_expiry")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn private_writer_preserves_identity_audit_capacity_and_atomicity() {
    let _serial = TEST.lock().await;
    let pool = owner().await;
    let actor = Capability::generate().unwrap().storage_hash();
    let token = Capability::generate().unwrap().storage_hash();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity) SELECT $1,$1,$1,$1,n,n,n,n,n+3600,gen_random_uuid() FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) t")
        .bind(token.as_slice()).execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT used_bytes FROM restrictions.capacity WHERE singleton")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    for (days, dmca, warning, permanent) in [
        (0, false, true, false),
        (-1, false, false, true),
        (3, true, false, false),
    ] {
        let id = record(&mut tx, &actor, Some(&token), days, dmca)
            .await
            .unwrap();
        let row: (bool,bool,bool,bool,String,bool,i64)=sqlx::query_as("SELECT global,warning,ends_at IS NULL,post_id IS NULL AND account_id IS NULL,origin,automatic_identity IS NOT NULL,charged_bytes FROM restrictions.bans WHERE id=$1")
            .bind(id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(
            row,
            (
                true,
                warning,
                permanent,
                true,
                if dmca { "dmca" } else { "file" }.into(),
                true,
                539
            )
        );
        let audit: (String, bool) = sqlx::query_as(
            "SELECT action,account_id IS NULL FROM restrictions.audit WHERE ban_id=$1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            audit,
            (if warning { "warning" } else { "ban" }.into(), true)
        );
    }
    let used: i64 =
        sqlx::query_scalar("SELECT used_bytes FROM restrictions.capacity WHERE singleton")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(used, before + 3 * 539);
    let anonymous = record(&mut tx, &actor, None, 1, false).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT automatic_identity IS NULL FROM restrictions.bans WHERE id=$1"
        )
        .bind(anonymous)
        .fetch_one(&mut *tx)
        .await
        .unwrap()
    );
    // Refusal must not partially charge capacity or insert a row/audit record.
    let snapshot: serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_array((SELECT used_bytes FROM restrictions.capacity WHERE singleton),(SELECT count(*) FROM restrictions.bans),(SELECT count(*) FROM restrictions.audit))")
        .fetch_one(&mut *tx).await.unwrap();
    for (days, dmca) in [(2, true), (-2, false), (3651, false)] {
        sqlx::query("SAVEPOINT rejected_effect")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = record(&mut tx, &actor, None, days, dmca).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        sqlx::query("ROLLBACK TO SAVEPOINT rejected_effect")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for limit in [
        "UPDATE restrictions.capacity SET byte_limit=used_bytes WHERE singleton",
        "UPDATE restrictions.capacity SET row_limit=(SELECT count(*) FROM restrictions.bans) WHERE singleton",
    ] {
        sqlx::query("SAVEPOINT full_capacity")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(limit).execute(&mut *tx).await.unwrap();
        sqlx::query("SAVEPOINT rejected_effect")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = record(&mut tx, &actor, None, 1, false).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("P134C")
        );
        sqlx::query("ROLLBACK TO SAVEPOINT full_capacity")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let after: serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_array((SELECT used_bytes FROM restrictions.capacity WHERE singleton),(SELECT count(*) FROM restrictions.bans),(SELECT count(*) FROM restrictions.audit))")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(after, snapshot);
    let exposed: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname NOT IN('board_migrator','board_restriction_owner') AND (has_function_privilege(oid,'restrictions.file_expiry(timestamptz,integer,text)','EXECUTE') OR has_function_privilege(oid,'restrictions.record_file_ban(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text)','EXECUTE')))")
        .fetch_one(&mut *tx).await.unwrap();
    assert!(!exposed);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn runtime_roles_and_repeatable_read_cannot_issue_private_file_bans() {
    let _serial = TEST.lock().await;
    for variable in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        for query in [
            "SELECT restrictions.file_expiry(now(),0,'UTC')",
            "SELECT restrictions.record_file_ban(decode(repeat('18',32),'hex'),NULL,'a',189,decode(repeat('42',16),'hex'),0,false,'Public reason','Private reason','UTC')",
        ] {
            let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "{variable}"
            );
        }
        runtime.close().await;
    }
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let actor = Capability::generate().unwrap().storage_hash();
    let error = record(&mut tx, &actor, None, 1, false).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("P134I")
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn private_writer_contention_refuses_without_partial_effects() {
    let _serial = TEST.lock().await;
    let pool = owner().await;
    let mut first_actor = Capability::generate().unwrap().storage_hash();
    let mut second_actor = Capability::generate().unwrap().storage_hash();
    first_actor[..2].copy_from_slice(&[0, 0]);
    second_actor[..2].copy_from_slice(&[0, 1]);
    let mut first = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *first)
        .await
        .unwrap();
    let first_id = record(&mut first, &first_actor, None, 1, false)
        .await
        .unwrap();
    let mut second = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *second)
        .await
        .unwrap();
    let error = record(&mut second, &second_actor, None, 1, false)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    second.rollback().await.unwrap();
    first.commit().await.unwrap();
    let mut retry = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *retry)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM restrictions.bans WHERE actor_hash=$1")
            .bind(second_actor.as_slice())
            .fetch_one(&mut *retry)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let second_id = record(&mut retry, &second_actor, None, 1, false)
        .await
        .unwrap();
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM restrictions.audit WHERE ban_id=ANY($1)")
            .bind(vec![first_id, second_id])
            .fetch_one(&mut *retry)
            .await
            .unwrap();
    assert_eq!(rows, 2);
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id=ANY($1)")
        .bind(vec![first_id, second_id])
        .execute(&mut *retry)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.bans WHERE id=ANY($1)")
        .bind(vec![first_id, second_id])
        .execute(&mut *retry)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-1078 WHERE singleton")
        .execute(&mut *retry)
        .await
        .unwrap();
    retry.commit().await.unwrap();
}
