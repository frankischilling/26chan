#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use sqlx::{PgPool, Postgres, Transaction};

static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn owner() -> PgPool {
    PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap()
}

async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    actor: &[u8],
    origin: &str,
    staff: (Option<i64>, Option<i64>),
    file: (Option<i64>, Option<&[u8]>),
    warning: bool,
    offset: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest) VALUES('a',$1,$8,true,$2,date_trunc('second',statement_timestamp()),date_trunc('second',statement_timestamp())+make_interval(secs=>$3::double precision),'Public synthetic reason','Auto-ban: Private synthetic reason',$4,1024,$5,$6,$7) RETURNING id")
        .bind(staff.0).bind(warning).bind(offset).bind(staff.1).bind(origin).bind(file.0).bind(file.1).bind(actor).fetch_one(&mut **tx).await
}

#[tokio::test]
async fn typed_provenance_preserves_staff_and_private_authority() {
    let _serial = TEST.lock().await;
    let actor = Capability::generate().unwrap().storage_hash();
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT content.restrictions_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap()
    );
    for (origin, post, account, rule, digest, warning, offset, valid) in [
        ("staff", Some(1), Some(1), None, None, false, 60, true),
        ("staff", Some(1), Some(1), None, None, true, 0, true),
        ("staff", Some(1), Some(1), None, None, true, -1, false),
        ("staff", None, Some(1), None, None, false, 60, false),
        ("staff", Some(1), None, None, None, false, 60, false),
        (
            "staff",
            Some(1),
            Some(1),
            Some(1),
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            false,
            60,
            true,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            true,
            0,
            true,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            true,
            -3600,
            true,
        ),
        (
            "dmca",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            false,
            259200,
            true,
        ),
        (
            "file",
            Some(1),
            None,
            Some(1),
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            Some(1),
            Some(1),
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            None,
            None,
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
        ("file", None, None, Some(1), None, false, 60, false),
        (
            "file",
            None,
            None,
            Some(0),
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 15]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 17]),
            false,
            60,
            false,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            false,
            -1,
            false,
        ),
        (
            "file",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            true,
            60,
            false,
        ),
        (
            "other",
            None,
            None,
            Some(1),
            Some(vec![1; 16]),
            false,
            60,
            false,
        ),
    ] {
        sqlx::query("SAVEPOINT provenance_case")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = insert(
            &mut tx,
            &actor,
            origin,
            (post, account),
            (rule, digest.as_deref()),
            warning,
            offset,
        )
        .await;
        if valid {
            assert!(result.is_ok(), "{origin}: {result:?}");
        } else {
            assert_eq!(
                result
                    .unwrap_err()
                    .as_database_error()
                    .unwrap()
                    .code()
                    .as_deref(),
                Some("23514"),
                "{origin}"
            );
        }
        sqlx::query("ROLLBACK TO SAVEPOINT provenance_case")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let roles: Vec<String> = sqlx::query_scalar("SELECT rolname::text FROM pg_roles WHERE rolname LIKE 'board_%' AND rolname NOT IN('board_migrator','board_restriction_owner')")
        .fetch_all(&mut *tx).await.unwrap();
    for role in roles {
        let allowed: bool = sqlx::query_scalar(
            "SELECT has_table_privilege($1,'restrictions.bans','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') OR has_any_column_privilege($1,'restrictions.bans','SELECT,INSERT,UPDATE,REFERENCES') OR has_function_privilege($1,'restrictions.posting_decision(bytea,bytea,text,bigint)','EXECUTE')",
        )
        .bind(&role)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert!(!allowed, "Unexpected direct restriction access: {role}");
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn automatic_warnings_use_existing_status_acknowledgement_and_audit() {
    let _serial = TEST.lock().await;
    let actor = Capability::generate().unwrap().storage_hash();
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let id = insert(
        &mut tx,
        &actor,
        "file",
        (None, None),
        (Some(188), Some(&[42; 16])),
        true,
        -3600,
    )
    .await
    .unwrap();
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = public.begin().await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT content.evaluate_posting_restrictions($1,NULL,'a',$2)")
            .bind(actor.as_slice())
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(state, "warned");
    let status: Vec<(i64, bool, String)> = sqlx::query_as(
        "SELECT id,warning,public_reason FROM content.restriction_status($1,NULL,'a',$2)",
    )
    .bind(actor.as_slice())
    .bind(now)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(status, vec![(id, true, "Public synthetic reason".into())]);
    let wrong_actor: bool =
        sqlx::query_scalar("SELECT content.acknowledge_warning($1,NULL,'a',$2,$3)")
            .bind([0x19u8; 32].as_slice())
            .bind(now)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(!wrong_actor);
    let acknowledged: bool =
        sqlx::query_scalar("SELECT content.acknowledge_warning($1,NULL,'a',$2,$3)")
            .bind(actor.as_slice())
            .bind(now)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(acknowledged);
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let lifecycle: (bool,String,bool,bool) = sqlx::query_as("SELECT active,retirement,post_id IS NULL,account_id IS NULL FROM restrictions.bans WHERE id=$1")
        .bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(lifecycle, (false, "warning-viewed".into(), true, true));
    let audit: Vec<(String, bool)> =
        sqlx::query_as("SELECT action,account_id IS NULL FROM restrictions.audit WHERE ban_id=$1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(audit, vec![("warning-viewed".into(), true)]);
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.bans WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn automatic_proxy_exception_requires_real_verification_and_address_match() {
    let _serial = TEST.lock().await;
    let actor = Capability::generate().unwrap().storage_hash();
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    let token = [0x87u8; 32];
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,verified_level,automatic_identity) VALUES($1,$1,$1,$1,$2-10,$2-10,$2-10,$2-10,$2+3600,1,gen_random_uuid())")
        .bind(token.as_slice()).bind(now).execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let id = insert(
        &mut tx,
        &actor,
        "file",
        (None, None),
        (Some(188), Some(&[42; 16])),
        false,
        3600,
    )
    .await
    .unwrap();
    for (public, private, expected) in [
        ("Proxy public", "Synthetic", "allowed"),
        ("Synthetic", "Proxy private", "allowed"),
        ("proxy public", "Synthetic", "banned"),
        ("Synthetic", "proxy private", "banned"),
    ] {
        sqlx::query("UPDATE restrictions.bans SET public_reason=$1,private_reason=$2 WHERE id=$3")
            .bind(public)
            .bind(private)
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let state: String = sqlx::query_scalar(
            "SELECT disposition FROM restrictions.posting_decision($1,$2,'a',$3)",
        )
        .bind(actor.as_slice())
        .bind(token.as_slice())
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(state, expected);
    }
    sqlx::query("UPDATE restrictions.bans SET public_reason='Proxy public',automatic_identity=(SELECT automatic_identity FROM post_secrets.lookup_automatic_identity($1,$2)) WHERE id=$3")
        .bind(token.as_slice()).bind(now).bind(id).execute(&mut *tx).await.unwrap();
    let identity_state: String =
        sqlx::query_scalar("SELECT disposition FROM restrictions.posting_decision($1,$2,'a',$3)")
            .bind(actor.as_slice())
            .bind(token.as_slice())
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        identity_state, "banned",
        "Verified bypass applies only to an address match"
    );
    sqlx::query("UPDATE restrictions.bans SET automatic_identity=NULL WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let absent: String =
        sqlx::query_scalar("SELECT disposition FROM restrictions.posting_decision($1,NULL,'a',$2)")
            .bind(actor.as_slice())
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(absent, "banned");
    sqlx::query("UPDATE restrictions.bans SET origin='staff',post_id=1,account_id=1,file_rule_id=NULL,file_digest=NULL WHERE id=$1")
        .bind(id).execute(&mut *tx).await.unwrap();
    let staff: String =
        sqlx::query_scalar("SELECT disposition FROM restrictions.posting_decision($1,$2,'a',$3)")
            .bind(actor.as_slice())
            .bind(token.as_slice())
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        staff, "banned",
        "Staff bans cannot claim automatic provenance"
    );
    tx.rollback().await.unwrap();
}
