#![cfg(feature = "database-tests")]

use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};

// Catalog fixtures never commit. Revision history is append-only production
// configuration, so tests must not consume its bounded permanent capacity.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool(variable: &str, expected_role: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, expected_role, "{variable} must use its actual login");
    pool
}

fn row(id: i64) -> Value {
    json!({"id": id, "board": "", "op_only": false, "reply_only": false,
        "image_only": false, "exclude_boards": null, "title": "Synthetic category",
        "weight": 1, "filtered": 0})
}

fn catalog(rows: Vec<Value>) -> Value {
    json!({"version": 1, "categories": rows})
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

async fn import(connection: &mut PgConnection, value: &Value) -> Result<i64, sqlx::Error> {
    import_text(connection, &value.to_string()).await
}

async fn import_text(connection: &mut PgConnection, value: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
        .bind(value)
        .fetch_one(connection)
        .await
}

async fn read(connection: &mut PgConnection, revision: i64) -> Result<Value, sqlx::Error> {
    let value: String = sqlx::query_scalar("SELECT content.read_report_catalog($1)::text")
        .bind(revision)
        .fetch_one(connection)
        .await?;
    Ok(serde_json::from_str(&value).unwrap())
}

async fn rejects(connection: &mut PgConnection, value: &Value, expected: &str) {
    // Recover the transaction after the intentional SQL error, retaining earlier
    // imported revisions so an invalid payload cannot quietly replace them.
    sqlx::query("SAVEPOINT invalid_catalog")
        .execute(&mut *connection)
        .await
        .unwrap();
    let error = import(connection, value).await.unwrap_err();
    assert_eq!(code(&error), expected);
    sqlx::query("ROLLBACK TO SAVEPOINT invalid_catalog")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("RELEASE SAVEPOINT invalid_catalog")
        .execute(connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn ordered_revisions_roundtrip_null_empty_and_raw_configuration() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL extra_float_digits = -3")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut rows = vec![row(90), row(3), row(31), row(i64::MAX)];
    rows[0]["board"] = Value::Null;
    rows[0]["filtered"] = json!(i64::MIN);
    rows[0]["weight"] = json!(-0.125);
    rows[1]["exclude_boards"] = json!("");
    rows[1]["title"] = json!("");
    rows[2]["board"] = json!("0");
    rows[2]["exclude_boards"] = json!(" fixture,FIXTURE,fixture ,0,");
    rows[2]["op_only"] = json!(true);
    rows[2]["reply_only"] = json!(true);
    rows[2]["image_only"] = json!(true);
    rows[3]["board"] = json!("_ws_");
    rows[3]["filtered"] = json!(i64::MAX);
    rows[3]["weight"] = json!(1.2345678901234567);
    let original = catalog(rows);
    let first = import(&mut tx, &original).await.unwrap();
    assert!(first > 0);
    assert_eq!(read(&mut tx, first).await.unwrap(), original);
    let empty = catalog(vec![]);
    let second = import(&mut tx, &empty).await.unwrap();
    assert!(second > first);
    assert_eq!(read(&mut tx, second).await.unwrap(), empty);
    let mut zero = row(1);
    zero["weight"] = json!(-0.0);
    let zero_revision = import(&mut tx, &catalog(vec![zero])).await.unwrap();
    let read_zero = read(&mut tx, zero_revision).await.unwrap();
    // JSONB normalizes the sign of zero; the contract is numeric equality.
    assert_eq!(read_zero["categories"][0]["weight"].as_f64().unwrap(), 0.0);

    assert_eq!(read(&mut tx, first).await.unwrap(), original);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn invalid_envelopes_and_fields_are_rejected_atomically() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let original = catalog(vec![row(17)]);
    let revision = import(&mut tx, &original).await.unwrap();
    for invalid in [
        Value::Null,
        json!([]),
        json!(true),
        json!({}),
        json!({"version": 1}),
        json!({"categories": []}),
        json!({"version": 2, "categories": []}),
        json!({"version": "1", "categories": []}),
        json!({"version": 1, "categories": null}),
        json!({"version": 1, "categories": {}}),
        json!({"version": 1, "categories": [], "extra": true}),
        catalog(vec![Value::Null]),
        catalog(vec![json!([])]),
        catalog(vec![row(1), row(1)]),
    ] {
        rejects(&mut tx, &invalid, "22023").await;
    }
    for field in [
        "id",
        "board",
        "op_only",
        "reply_only",
        "image_only",
        "exclude_boards",
        "title",
        "weight",
        "filtered",
    ] {
        let mut missing = row(1);
        missing.as_object_mut().unwrap().remove(field);
        rejects(&mut tx, &catalog(vec![missing]), "22023").await;
    }
    let mut extra = row(1);
    extra["unknown"] = json!(0);
    rejects(&mut tx, &catalog(vec![extra]), "22023").await;
    for (field, values) in [
        (
            "id",
            vec![
                Value::Null,
                json!(0),
                json!(-1),
                json!(1.5),
                json!("1"),
                json!(u64::MAX),
            ],
        ),
        ("board", vec![json!(1), json!(false), json!([])]),
        ("exclude_boards", vec![json!([]), json!(false)]),
        ("title", vec![Value::Null, json!(1), json!([])]),
        (
            "weight",
            vec![Value::Null, json!("NaN"), json!("Infinity"), json!(true)],
        ),
        (
            "filtered",
            vec![Value::Null, json!(1.5), json!("0"), json!(u64::MAX)],
        ),
        ("op_only", vec![Value::Null, json!(0), json!("false")]),
        ("reply_only", vec![Value::Null, json!(1), json!("true")]),
        ("image_only", vec![Value::Null, json!(0), json!([])]),
    ] {
        for value in values {
            let mut invalid = row(1);
            invalid[field] = value;
            rejects(&mut tx, &catalog(vec![invalid]), "22023").await;
        }
    }
    // Valid JSONB numerics can exceed finite float8 without being JSON strings.
    sqlx::query("SAVEPOINT overflow_weight")
        .execute(&mut *tx)
        .await
        .unwrap();
    let huge_weight = original
        .to_string()
        .replace("\"weight\":1", "\"weight\":1e400");
    assert_ne!(huge_weight, original.to_string());
    assert_eq!(
        code(&import_text(&mut tx, &huge_weight).await.unwrap_err()),
        "22023"
    );
    sqlx::query("ROLLBACK TO SAVEPOINT overflow_weight")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(read(&mut tx, revision).await.unwrap(), original);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn field_byte_row_and_normalized_jsonb_limits_are_enforced() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let mut boundary = row(1);
    // Multibyte text ensures limits count UTF-8 bytes rather than characters.
    boundary["board"] = json!("é".repeat(128));
    boundary["exclude_boards"] = json!("é".repeat(32768));
    boundary["title"] = json!("é".repeat(2048));
    let valid = catalog(vec![boundary.clone()]);
    let revision = import(&mut tx, &valid).await.unwrap();
    assert_eq!(read(&mut tx, revision).await.unwrap(), valid);
    for field in ["board", "exclude_boards", "title"] {
        let mut oversized = boundary.clone();
        oversized[field] = json!(format!("{}x", oversized[field].as_str().unwrap()));
        rejects(&mut tx, &catalog(vec![oversized]), "22023").await;
    }
    let maximum_rows = catalog((1..=4096).map(row).collect());
    let revision = import(&mut tx, &maximum_rows).await.unwrap();
    assert_eq!(read(&mut tx, revision).await.unwrap(), maximum_rows);
    rejects(&mut tx, &catalog((1..=4097).map(row).collect()), "22023").await;
    let huge = catalog(
        (1..=129)
            .map(|id| {
                let mut value = row(id);
                value["exclude_boards"] = json!("x".repeat(65536));
                value
            })
            .collect(),
    );
    let normalized_bytes: i32 = sqlx::query_scalar("SELECT octet_length(($1::text::jsonb)::text)")
        .bind(huge.to_string())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(normalized_bytes > 8 * 1024 * 1024);
    rejects(&mut tx, &huge, "22023").await;
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn private_owner_and_actual_runtime_logins_cannot_bypass_api_boundary() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let objects: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname,r.rolname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_roles r ON r.oid=c.relowner WHERE n.nspname='post_secrets' \
         AND c.relname IN ('report_catalog_gate','report_catalog_versions','report_catalog_rows') ORDER BY c.relname",
    ).fetch_all(&owner).await.unwrap();
    assert_eq!(objects.len(), 3);
    for (_, role) in objects {
        assert_eq!(role, "board_report_admission_owner");
    }
    let functions: Vec<(String, bool, String, Vec<String>)> = sqlx::query_as(
        "SELECT p.proname,p.prosecdef,r.rolname,p.proconfig FROM pg_proc p \
         JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_roles r ON r.oid=p.proowner \
         WHERE n.nspname='content' AND p.proname IN ('import_report_catalog','read_report_catalog')",
    ).fetch_all(&owner).await.unwrap();
    assert_eq!(functions.len(), 2);
    for (_, definer, role, config) in functions {
        assert!(definer);
        assert!(config.iter().any(|value| value == "extra_float_digits=3"));
        assert_eq!(role, "board_report_admission_owner");
        assert!(
            config
                .iter()
                .any(|value| value == "search_path=pg_catalog, pg_temp"
                    || value == "search_path=pg_catalog,pg_temp")
        );
    }
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
        ("MIGRATION_DATABASE_URL", "board_migrator"),
    ] {
        let login = pool(variable, role).await;
        for (table, read_sql) in [
            (
                "report_catalog_gate",
                "SELECT * FROM post_secrets.report_catalog_gate LIMIT 0",
            ),
            (
                "report_catalog_versions",
                "SELECT * FROM post_secrets.report_catalog_versions LIMIT 0",
            ),
            (
                "report_catalog_rows",
                "SELECT * FROM post_secrets.report_catalog_rows LIMIT 0",
            ),
        ] {
            for privilege in [
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "TRUNCATE",
                "REFERENCES",
                "TRIGGER",
            ] {
                let permitted: bool =
                    sqlx::query_scalar("SELECT has_table_privilege(current_user,c.oid,$2) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='post_secrets' AND c.relname=$1")
                        .bind(table)
                        .bind(privilege)
                        .fetch_one(&login)
                        .await
                        .unwrap();
                assert!(!permitted, "{role} {table} {privilege}");
            }
            // Exercise real table access, rather than relying only on ACL metadata.
            let error = sqlx::query(read_sql).execute(&login).await.unwrap_err();
            assert_eq!(code(&error), "42501");
        }
        if role != "board_migrator" {
            for statement in [
                "SELECT content.import_report_catalog('{\"version\":1,\"categories\":[]}'::jsonb)",
                "SELECT content.read_report_catalog(1)",
            ] {
                let error = sqlx::query(statement).execute(&login).await.unwrap_err();
                assert_eq!(code(&error), "42501", "{role}: {statement}");
            }
            let error = sqlx::query("SET ROLE board_report_admission_owner")
                .execute(&login)
                .await
                .unwrap_err();
            assert_eq!(code(&error), "42501");
        }
        login.close().await;
    }
    // Even the function owner fails the invoker guard: migration membership
    // must not become an alternate callable import or read API.
    for statement in [
        "SELECT content.import_report_catalog('{\"version\":1,\"categories\":[]}'::jsonb)",
        "SELECT content.read_report_catalog(1)",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            code(&sqlx::query(statement).execute(&mut *tx).await.unwrap_err()),
            "42501"
        );
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_revision_missing_revision_and_isolation_errors_are_distinct() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    for revision in [None, Some(0_i64), Some(-1), Some(65), Some(i64::MAX)] {
        let error = sqlx::query("SELECT content.read_report_catalog($1)")
            .bind(revision)
            .execute(&owner)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "22023");
    }
    let mut tx = owner.begin().await.unwrap();
    // Reserve an uncommitted revision, then roll back its savepoint to obtain a
    // guaranteed missing revision without assuming pristine global history.
    sqlx::query("SAVEPOINT missing_revision")
        .execute(&mut *tx)
        .await
        .unwrap();
    let missing = import(&mut tx, &catalog(vec![])).await.unwrap();
    sqlx::query("ROLLBACK TO SAVEPOINT missing_revision")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(code(&read(&mut tx, missing).await.unwrap_err()), "P0002");
    tx.rollback().await.unwrap();
    for isolation_sql in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(isolation_sql).execute(&mut *tx).await.unwrap();
        assert_eq!(
            code(&import(&mut tx, &catalog(vec![])).await.unwrap_err()),
            "22023"
        );
        tx.rollback().await.unwrap();
    }
    let mut tx = owner.begin().await.unwrap();
    assert_eq!(
        code(
            &sqlx::query("SELECT content.import_report_catalog(NULL)")
                .execute(&mut *tx)
                .await
                .unwrap_err()
        ),
        "22023"
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn capacity_exhaustion_and_missing_gate_fail_closed_without_retaining_fixtures() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let first = import(&mut tx, &catalog(vec![row(1)])).await.unwrap();
    for expected in first + 1..=64 {
        assert_eq!(import(&mut tx, &catalog(vec![])).await.unwrap(), expected);
    }
    rejects(&mut tx, &catalog(vec![]), "P0098").await;
    assert_eq!(read(&mut tx, first).await.unwrap(), catalog(vec![row(1)]));
    assert_eq!(
        read(&mut tx, 64).await.unwrap(),
        if first == 64 {
            catalog(vec![row(1)])
        } else {
            catalog(vec![])
        }
    );
    tx.rollback().await.unwrap();
    // The sole privileged mutation is a scoped availability fault. Rollback
    // restores the singleton; no production revision or category is modified.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.report_catalog_gate WHERE singleton")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    assert_eq!(
        code(&import(&mut tx, &catalog(vec![])).await.unwrap_err()),
        "P0098"
    );
    tx.rollback().await.unwrap();
    // A normal import still works after both deliberately failed transactions.
    let mut tx = owner.begin().await.unwrap();
    assert_eq!(import(&mut tx, &catalog(vec![])).await.unwrap(), first);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn importing_inactive_configuration_preserves_existing_report_admission() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    const DEFINITIONS: &str = "SELECT pg_get_functiondef(p.oid) FROM pg_proc p \
        JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='content' \
        AND p.proname IN ('admit_report','check_report_admission') ORDER BY p.oid";
    let before: Vec<String> = sqlx::query_scalar(DEFINITIONS)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert!(!before.is_empty());
    let mut illegal = row(31);
    illegal["title"] = json!("Inactive synthetic illegal label");
    illegal["weight"] = json!(-100);
    let revision = import(&mut tx, &catalog(vec![illegal])).await.unwrap();
    assert!(revision > 0);
    let after: Vec<String> = sqlx::query_scalar(DEFINITIONS)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(before, after);
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
    ] {
        let runtime = pool(variable, role).await;
        let ready: bool = sqlx::query_scalar(board_store::report_admission::READINESS_SQL)
            .fetch_one(&runtime)
            .await
            .unwrap();
        assert!(
            ready,
            "inactive catalogs must not alter {role} admission readiness"
        );
        runtime.close().await;
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn concurrent_import_waits_for_gate_and_rollback_releases_capacity() {
    let _serial = TEST.lock().await;
    let owner = pool("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut first = owner.begin().await.unwrap();
    let first_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first)
        .await
        .unwrap();
    let released_revision = import(&mut first, &catalog(vec![row(91)])).await.unwrap();
    let contender_pool = owner.clone();
    let (started, receiver) = tokio::sync::oneshot::channel();
    let contender = tokio::spawn(async move {
        let mut second = contender_pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *second)
            .await
            .unwrap();
        started.send(pid).unwrap();
        let value = catalog(vec![row(92)]);
        let revision = import(&mut second, &value).await.unwrap();
        assert_eq!(read(&mut second, revision).await.unwrap(), value);
        second.rollback().await.unwrap();
        revision
    });
    let contender_pid = receiver.await.unwrap();
    // Observe the actual blocking edge instead of guessing that a sleep was
    // long enough for the competing transaction to reach the import gate.
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waits_for_first: bool = sqlx::query_scalar("SELECT $1 = ANY(pg_blocking_pids($2))")
                .bind(first_pid)
                .bind(contender_pid)
                .fetch_one(&owner)
                .await
                .unwrap();
            if waits_for_first {
                break;
            }
            assert!(
                !contender.is_finished(),
                "contending import bypassed the gate"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    // Release the lock even if the observation assertion failed.
    first.rollback().await.unwrap();
    blocked.expect("catalog importer never waited on the catalog gate");
    let reclaimed = tokio::time::timeout(std::time::Duration::from_secs(10), contender)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed, released_revision);
    let mut after = owner.begin().await.unwrap();
    assert_eq!(
        import(&mut after, &catalog(vec![])).await.unwrap(),
        released_revision
    );
    after.rollback().await.unwrap();
}
