#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use sqlx::{PgPool, Postgres, Transaction};
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn owner() -> PgPool {
    PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap()
}
async fn role(tx: &mut Transaction<'_, Postgres>, restricted: bool) {
    sqlx::query(if restricted {
        "SET LOCAL ROLE board_restriction_owner"
    } else {
        "RESET ROLE"
    })
    .execute(&mut **tx)
    .await
    .unwrap();
}
fn code(e: sqlx::Error, expected: &str) {
    assert_eq!(
        e.as_database_error().unwrap().code().as_deref(),
        Some(expected)
    );
}
async fn resolve(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    revision: i64,
    normalized: Option<&str>,
    original: Option<&str>,
    eligible: Option<bool>,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    sqlx::query_as("SELECT rule_id,encode(digest,'hex') FROM admission.resolve_file_effect_rule($1,$2,decode($3,'hex'),decode($4,'hex'),$5)")
 .bind(board).bind(revision).bind(normalized).bind(original).bind(eligible).fetch_optional(&mut **tx).await
}
#[tokio::test]
async fn private_rule_resolution_matches_source_and_refuses_missing_or_ambiguous_context() {
    let _serial = TEST.lock().await;
    let pool = owner().await;
    let mut tx = pool.begin().await.unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../domain/tests/fixtures/file-filter-reference.json"
    ))
    .unwrap();
    let cases = fixture["exact_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2358);
    let mut qualified = 0;
    for case in cases {
        // Source no-file cases are outside this private resolver comparison.
        // The eventual posting caller still needs its own no-file test. NULL
        // on an existing upload means missing trusted metadata below.
        if case["normalized"].is_null() {
            assert!(case["matched"].is_null());
            assert_eq!(case["result"]["outcome"], "allow");
            continue;
        }
        qualified += 1;
        role(&mut tx, false).await;
        sqlx::query("DELETE FROM admission.file_rules")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET worksafe=$2 WHERE slug=$1")
            .bind(case["board"].as_str().unwrap())
            .bind(case["worksafe"].as_bool().unwrap())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO admission.file_rules(id,active,board,digest,mode,ban_days,quiet,public_reason,description) SELECT (r->>'id')::bigint,(r->>'active')::boolean,r->>'boardrestrict',decode(r->>'contents','hex'),(r->>'ban')::smallint,CASE WHEN (r->>'ban')::int=1 THEN (r->>'banlength')::int ELSE 0 END,(r->>'quiet')::boolean,r->>'banreason',r->>'description' FROM jsonb_array_elements($1::jsonb) r WHERE r->>'field'='md5'").bind(&case["rules"]).execute(&mut *tx).await.unwrap();
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM admission.policy WHERE singleton")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        role(&mut tx, true).await;
        let result = resolve(
            &mut tx,
            case["board"].as_str().unwrap(),
            revision,
            case["normalized"].as_str(),
            case["original"].as_str(),
            Some(!case["original"].is_null()),
        )
        .await;
        // The source's empty original value means known absence. The private bridge
        // instead receives NULL only for unavailable metadata on an existing upload.
        let result = result.unwrap_or_else(|e| panic!("{case}: {e}"));
        let expected = case["matched"].as_object().map(|m| {
            (
                m["rule"].as_i64().unwrap(),
                m["digest"].as_str().unwrap().to_owned(),
            )
        });
        assert_eq!(result, expected, "{case}");
    }
    assert_eq!(qualified, 2352);
    role(&mut tx, false).await;
    sqlx::query("DELETE FROM admission.file_rules")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.file_rules(id,digest,mode,ban_days,public_reason,description) VALUES(191,decode(repeat('11',16),'hex'),1,3,'Owned public reason','Owned private description')").execute(&mut *tx).await.unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy WHERE singleton")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    role(&mut tx, true).await;
    let row:(String,String)=sqlx::query_as("SELECT public_reason,description FROM admission.resolve_file_effect_rule('a',$1,decode(repeat('11',16),'hex'),NULL,NULL)").bind(revision).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        row,
        (
            "Owned public reason".into(),
            "Owned private description".into()
        )
    );
    for (n, o, e, rev) in [
        (
            None,
            Some("11111111111111111111111111111111"),
            Some(true),
            revision,
        ),
        (
            Some("22222222222222222222222222222222"),
            None,
            Some(true),
            revision,
        ),
        (
            Some("22222222222222222222222222222222"),
            Some("11111111111111111111111111111111"),
            None,
            revision,
        ),
        (
            Some("11111111111111111111111111111111"),
            None,
            None,
            revision - 1,
        ),
    ] {
        sqlx::query("SAVEPOINT unavailable")
            .execute(&mut *tx)
            .await
            .unwrap();
        code(
            resolve(&mut tx, "a", rev, n, o, e).await.unwrap_err(),
            "55000",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT unavailable")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    assert_eq!(
        resolve(
            &mut tx,
            "a",
            revision,
            Some("22222222222222222222222222222222"),
            None,
            Some(false)
        )
        .await
        .unwrap(),
        None
    );
    role(&mut tx, false).await;
    sqlx::query(
        "INSERT INTO admission.file_rules(id,digest) VALUES(192,decode(repeat('11',16),'hex'))",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy WHERE singleton")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    role(&mut tx, true).await;
    sqlx::query("SAVEPOINT ambiguous")
        .execute(&mut *tx)
        .await
        .unwrap();
    code(
        resolve(
            &mut tx,
            "a",
            revision,
            Some("11111111111111111111111111111111"),
            None,
            None,
        )
        .await
        .unwrap_err(),
        "55000",
    );
    sqlx::query("ROLLBACK TO SAVEPOINT ambiguous")
        .execute(&mut *tx)
        .await
        .unwrap();
    role(&mut tx, false).await;
    sqlx::query("DELETE FROM admission.file_rules")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.file_rules(id,digest) SELECT id,decode(repeat('33',16),'hex') FROM generate_series(1,129) id").execute(&mut *tx).await.unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy WHERE singleton")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    role(&mut tx, true).await;
    code(
        resolve(
            &mut tx,
            "a",
            revision,
            Some("11111111111111111111111111111111"),
            None,
            Some(false),
        )
        .await
        .unwrap_err(),
        "55000",
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}
#[tokio::test]
async fn private_effect_bridges_preserve_policy_locks_warning_atomicity_and_runtime_denials() {
    let _serial = TEST.lock().await;
    let pool = owner().await;
    let mut held = pool.begin().await.unwrap();
    role(&mut held, true).await;
    let exempt: bool = sqlx::query_scalar("SELECT admission.file_effect_exempt('192.0.2.191')")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    assert!(!exempt);
    let mut observer = pool.begin().await.unwrap();
    code(
        sqlx::query("SELECT singleton FROM admission.policy WHERE singleton FOR UPDATE NOWAIT")
            .execute(&mut *observer)
            .await
            .unwrap_err(),
        "55P03",
    );
    observer.rollback().await.unwrap();
    held.rollback().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) VALUES('192.0.2.0/24') ON CONFLICT(network) DO UPDATE SET active=true").execute(&mut *tx).await.unwrap();
    role(&mut tx, true).await;
    for (peer, expected) in [
        ("192.0.2.191", true),
        ("198.51.100.191", false),
        ("2001:db8::191", false),
    ] {
        let actual: bool = sqlx::query_scalar("SELECT admission.file_effect_exempt($1::inet)")
            .bind(peer)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(actual, expected);
    }
    let actor = Capability::generate().unwrap().storage_hash();
    for expected in [false, true] {
        let repeated: bool = sqlx::query_scalar("SELECT admission.file_effect_dmca($1)")
            .bind(actor.as_slice())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(repeated, expected);
    }
    // The caller's outer rollback also removes a newly recorded warning.
    tx.rollback().await.unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admission.dmca_warnings WHERE actor_hash=$1")
            .bind(actor.as_slice())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    for name in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(name).unwrap())
            .await
            .unwrap();
        for sql in [
            "SELECT * FROM admission.resolve_file_effect_rule('a',1,NULL,NULL,NULL)",
            "SELECT admission.file_effect_exempt('192.0.2.1')",
            "SELECT admission.file_effect_dmca(decode(repeat('11',32),'hex'))",
        ] {
            code(
                sqlx::query(sql).execute(&runtime).await.unwrap_err(),
                "42501",
            );
        }
        runtime.close().await;
    }
    let mut tx = pool.begin().await.unwrap();
    role(&mut tx, true).await;
    for sql in [
        "SELECT * FROM admission.file_rules",
        "SELECT * FROM admission.dmca_warnings",
        "SELECT * FROM admission.automatic_ban_exemptions",
    ] {
        sqlx::query("SAVEPOINT private_table")
            .execute(&mut *tx)
            .await
            .unwrap();
        code(
            sqlx::query(sql).execute(&mut *tx).await.unwrap_err(),
            "42501",
        );
        sqlx::query("ROLLBACK TO SAVEPOINT private_table")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.rollback().await.unwrap();
    pool.close().await;
}
