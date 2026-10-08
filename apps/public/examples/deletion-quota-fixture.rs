//! Test-only deletion and posting isolation for a finite owned browser run.
//! Both separately derived actors retain the same real key and loopback peer.
//! Build explicitly with --features browser-tests; never shipped as a server.
#![forbid(unsafe_code)]

use board_domain::poster_id::PosterIdKey;
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use std::{error::Error, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u8,
    key: String,
    marker: String,
    owner_pid: u32,
    database: String,
}

impl Manifest {
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.owner_pid == 0
            || !hex_token(&self.marker)
            || !hex_token(&self.key)
            || self.database.is_empty()
            || self.database.len() > 63
            || !self
                .database
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("Invalid owned quota manifest".into());
        }
        PosterIdKey::parse(&self.key)?;
        Ok(())
    }

    fn posting_actor(&self) -> Result<[u8; 32]> {
        Ok(*PosterIdKey::parse(&self.key)?
            .public_posting_rate_identity(std::net::Ipv4Addr::LOCALHOST.into())
            .as_bytes())
    }

    fn actor(&self) -> Result<[u8; 32]> {
        Ok(*PosterIdKey::parse(&self.key)?
            .public_deletion_rate_identity(std::net::Ipv4Addr::LOCALHOST.into())
            .as_bytes())
    }
}

fn hex_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && value.bytes().any(|b| b != b'0')
}

fn read_manifest(filename: &Path) -> Result<Manifest> {
    let metadata = std::fs::symlink_metadata(filename)?;
    let parent = filename
        .parent()
        .ok_or("Private manifest directory required")?;
    let directory = std::fs::symlink_metadata(parent)?;
    if !metadata.is_file() || metadata.len() > 4096 || !directory.is_dir() {
        return Err("Private manifest required".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o077 != 0
            || directory.mode() & 0o077 != 0
            || metadata.uid() != directory.uid()
            || metadata.nlink() != 1
        {
            return Err("Private manifest permissions required".into());
        }
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(filename)?)?;
    manifest.validate()?;
    Ok(manifest)
}

fn validate_target(environment: &str, database_url: &str, manifest: &Manifest) -> Result<()> {
    if environment != "development" {
        return Err("Explicit development mode required".into());
    }
    let url = url::Url::parse(database_url)?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str() != Some("127.0.0.1")
        || url.username() != "board_migrator"
        || url.path() != format!("/{}", manifest.database)
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Owned loopback migration database required".into());
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        // SQL errors and connection strings may contain secrets. Never print them.
        eprintln!("Owned deletion quota fixture failed");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty()
        || !matches!(
            args[0].as_str(),
            "init"
                | "reset"
                | "reset-posting"
                | "check"
                | "finish"
                | "catalog-enable"
                | "catalog-disable"
                | "catalog-inspect"
        )
        || (args[0] == "catalog-inspect" && args.len() != 3)
        || (args[0] != "catalog-inspect" && args.len() != 1)
    {
        return Err("One fixed fixture command required".into());
    }
    let manifest = read_manifest(Path::new(&std::env::var(
        "BROWSER_DELETION_QUOTA_MANIFEST",
    )?))?;
    let database_url = std::env::var("MIGRATION_DATABASE_URL")?;
    validate_target(&std::env::var("APP_ENV")?, &database_url, &manifest)?;
    let pool = PgPool::connect(&database_url).await?;
    let target = if args[0] == "catalog-inspect" {
        let op: i64 = args[1].parse()?;
        let target: i64 = args[2].parse()?;
        if op <= 0 || target <= 0 {
            return Err("Positive owned receipts required".into());
        }
        Some((op, target))
    } else {
        None
    };
    let result = apply_command(&pool, &manifest, &args[0], target).await;
    pool.close().await;
    result
}

#[cfg(test)]
async fn apply(pool: &PgPool, manifest: &Manifest, command: &str) -> Result<()> {
    apply_command(pool, manifest, command, None).await
}

async fn apply_command(
    pool: &PgPool,
    manifest: &Manifest,
    command: &str,
    target: Option<(i64, i64)>,
) -> Result<()> {
    manifest.validate()?;
    if !matches!(
        command,
        "init"
            | "reset"
            | "reset-posting"
            | "check"
            | "finish"
            | "catalog-enable"
            | "catalog-disable"
            | "catalog-inspect"
    ) {
        return Err("Unknown fixture command".into());
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL statement_timeout='10s'")
        .execute(&mut *tx)
        .await?;
    let identity: (String, String, String) = sqlx::query_as(
        "SELECT current_user::text,current_database()::text,pg_get_userbyid(datdba)::text FROM pg_database WHERE datname=current_database()",
    ).fetch_one(&mut *tx).await?;
    if identity
        != (
            "board_migrator".into(),
            manifest.database.clone(),
            "board_migrator".into(),
        )
    {
        return Err("Owned database migration identity required".into());
    }
    let fixture: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug='fixture' AND title='Test fixtures' AND description='Owned unfiltered posting fixtures.' AND deletion_known_min_seconds=0 AND deletion_unknown_min_seconds=0) AND EXISTS(SELECT 1 FROM content.posts WHERE board='demo' AND id=1000001 AND subject='What are you making?' AND created_at='2026-09-08 12:00:00+00'::timestamptz)",
    ).fetch_one(&mut *tx).await?;
    if !fixture {
        return Err("Expected disposable synthetic fixtures required".into());
    }
    // Coordinates cooperating fixture leases only. Other workloads must use
    // an exclusive disposable database; this cannot fence unrelated SQL.
    sqlx::query("SELECT pg_advisory_xact_lock(260085001)")
        .execute(&mut *tx)
        .await?;
    let mut output = None;
    if command == "init" {
        sqlx::query("CREATE TABLE IF NOT EXISTS post_secrets.browser_deletion_fixture_runs (marker text PRIMARY KEY CHECK(length(marker)=64),actor_hash bytea UNIQUE NOT NULL CHECK(octet_length(actor_hash)=32),database_name text NOT NULL,expires_at timestamptz NOT NULL,groups integer NOT NULL DEFAULT 0 CHECK(groups BETWEEN 0 AND 10000))")
            .execute(&mut *tx).await?;
        sqlx::query("REVOKE ALL ON post_secrets.browser_deletion_fixture_runs FROM PUBLIC")
            .execute(&mut *tx)
            .await?;
    }
    let owner: String = sqlx::query_scalar("SELECT pg_get_userbyid(relowner)::text FROM pg_class WHERE oid='post_secrets.browser_deletion_fixture_runs'::regclass")
        .fetch_one(&mut *tx).await?;
    if owner != "board_migrator" {
        return Err("Owned lease table required".into());
    }
    if command == "init" {
        sqlx::query("ALTER TABLE post_secrets.browser_deletion_fixture_runs ADD COLUMN IF NOT EXISTS posting_actor_hash bytea UNIQUE CHECK(octet_length(posting_actor_hash)=32), ADD COLUMN IF NOT EXISTS posting_groups integer NOT NULL DEFAULT 0 CHECK(posting_groups BETWEEN 0 AND 10000), ADD COLUMN IF NOT EXISTS catalog_revision bigint CHECK(catalog_revision BETWEEN 1 AND 64), ADD COLUMN IF NOT EXISTS catalog_groups integer NOT NULL DEFAULT 0 CHECK(catalog_groups BETWEEN 0 AND 10000)")
            .execute(&mut *tx).await?;
    }
    if command == "init" {
        let catalog_owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.browser_deletion_fixture_runs WHERE catalog_revision IS NOT NULL)")
            .fetch_one(&mut *tx).await?;
        if catalog_owned {
            return Err("Another catalog fixture lease exists".into());
        }
    }
    let actor = manifest.actor()?;
    let posting_actor = manifest.posting_actor()?;
    // Migrator membership is SET-only, not inherited. Assume the narrow function
    // owner only to acquire the normal application locks; restore migrator before
    // any lease or ledger operation. Transaction-local role and locks roll back
    // together if the function fails. No runtime grants are widened.
    sqlx::query("SET LOCAL ROLE board_posting_cooldown_owner")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT content.lock_posting_actor($1,true)")
        .bind(posting_actor.as_slice())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL ROLE board_migrator")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "SELECT singleton FROM post_secrets.public_deletion_capacity WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await?;
    if command == "init" {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.public_deletion_actors WHERE actor_hash=$1) OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE actor_hash=$2) OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE actor_hash=$2)",
        )
        .bind(actor.as_slice())
        .bind(posting_actor.as_slice())
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err("A fresh unused synthetic actor is required".into());
        }
        sqlx::query("INSERT INTO post_secrets.browser_deletion_fixture_runs(marker,actor_hash,database_name,posting_actor_hash,expires_at) VALUES($1,$2,$3,$4,now()+interval '12 hours')")
            .bind(&manifest.marker).bind(actor.as_slice()).bind(&manifest.database).bind(posting_actor.as_slice())
            .execute(&mut *tx).await?;
    } else {
        let lease: Option<(i32, i32)> = sqlx::query_as("SELECT groups,posting_groups FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1 AND actor_hash=$2 AND database_name=$3 AND posting_actor_hash=$4 AND expires_at>now() FOR UPDATE")
            .bind(&manifest.marker).bind(actor.as_slice()).bind(&manifest.database).bind(posting_actor.as_slice())
            .fetch_optional(&mut *tx).await?;
        let (groups, posting_groups) = lease.ok_or("Current exact ownership lease required")?;
        match command {
            "reset" => {
                if groups >= 10000 {
                    return Err("Finite fixture run exhausted".into());
                }
                remove_actor(&mut tx, &actor).await?;
                sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET groups=groups+1 WHERE marker=$1 AND actor_hash=$2")
                    .bind(&manifest.marker).bind(actor.as_slice()).execute(&mut *tx).await?;
            }
            "reset-posting" => {
                if posting_groups >= 10000 {
                    return Err("Finite posting fixture run exhausted".into());
                }
                remove_posting_actor(&mut tx, &posting_actor).await?;
                sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET posting_groups=posting_groups+1 WHERE marker=$1 AND posting_actor_hash=$2")
                    .bind(&manifest.marker).bind(posting_actor.as_slice()).execute(&mut *tx).await?;
            }
            "check" => {
                if groups == 0 {
                    return Err("Owned quota scope has not started".into());
                }
                let count: Option<i32> = sqlx::query_scalar("SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=$1 FOR UPDATE")
                    .bind(actor.as_slice()).fetch_optional(&mut *tx).await?;
                if count.unwrap_or(0) > 3 {
                    return Err("Quota scope exceeded three successful requests".into());
                }
            }
            "catalog-enable" | "catalog-disable" | "catalog-inspect" => {
                output = Some(catalog_command(&mut tx, manifest, command, target).await?);
            }
            "finish" => {
                // Clear only this lease's pointer before deleting its proof. Any
                // foreign pointer or uncertainty rolls back the entire finish.
                catalog_command(&mut tx, manifest, "catalog-disable", None).await?;
                remove_posting_actor(&mut tx, &posting_actor).await?;
                remove_actor(&mut tx, &actor).await?;
                let changed = sqlx::query("DELETE FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1 AND actor_hash=$2")
                    .bind(&manifest.marker).bind(actor.as_slice()).execute(&mut *tx).await?;
                if changed.rows_affected() != 1 {
                    return Err("Exactly one owned lease required".into());
                }
            }
            _ => return Err("Unknown fixture command".into()),
        }
    }
    tx.commit().await?;
    if let Some(value) = output {
        println!("{value}");
    }
    Ok(())
}

// Every field is synthetic and explicit. These are test fixtures, not recovered
// production categories. Import and lease recording share one transaction.
const SYNTHETIC_CATALOG: &str = r#"{"version":1,"categories":[
 {"id":9001,"board":"demo","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic board rule","weight":1.25,"filtered":0},
 {"id":31,"board":null,"op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic illegal content","weight":2.5,"filtered":0}
]}"#;

type CatalogReportRow = (Option<i64>, Option<i64>, Option<i16>, Option<f64>, String);

async fn catalog_command(
    tx: &mut Transaction<'_, Postgres>,
    manifest: &Manifest,
    command: &str,
    target: Option<(i64, i64)>,
) -> Result<serde_json::Value> {
    let (mut revision, activations): (Option<i64>, i32) = sqlx::query_as("SELECT catalog_revision,catalog_groups FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1 FOR UPDATE")
        .bind(&manifest.marker).fetch_one(&mut **tx).await?;
    if command == "catalog-inspect" {
        let (op, target) = target.ok_or("Owned OP and target required")?;
        // IDs alone are not authority: require the exact private run marker on
        // a recent live OP and a target in that exact demo thread.
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts op JOIN content.posts p ON p.board=op.board AND p.thread_id=op.id WHERE op.board='demo' AND op.id=$1 AND op.thread_id=op.id AND op.subject=$3 AND op.created_at>now()-interval '12 hours' AND NOT op.deleted AND p.id=$2 AND NOT p.deleted)")
            .bind(op).bind(target).bind(&manifest.marker).fetch_one(&mut **tx).await?;
        if !owned {
            return Err("Exact run-marked OP and target required".into());
        }
        let rows: Vec<CatalogReportRow> = sqlx::query_as("SELECT category_revision,category_id,category_kind,category_base_weight,reason FROM content.reports WHERE board='demo' AND post_id=$1 ORDER BY id LIMIT 33")
            .bind(target).fetch_all(&mut **tx).await?;
        if rows.len() > 32 {
            return Err("Owned report inspection bound exceeded".into());
        }
        return Ok(
            serde_json::json!({"reportCount":rows.len(),"categories":rows.into_iter().map(|(revision,id,kind,weight,title)| serde_json::json!({"revision":revision,"id":id,"kind":kind,"baseWeight":weight,"title":title})).collect::<Vec<_>>()}),
        );
    }
    if command == "catalog-enable" {
        let foreign: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.browser_deletion_fixture_runs WHERE marker<>$1)")
            .bind(&manifest.marker).fetch_one(&mut **tx).await?;
        if foreign {
            return Err("Exclusive catalog fixture lease required".into());
        }
        if activations >= 10000 {
            return Err("Finite catalog fixture run exhausted".into());
        }
    }
    // Migrator has SET-only membership; do not grant any runtime access.
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut **tx)
        .await?;
    let active: Option<i64> = sqlx::query_scalar("SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
        .fetch_one(&mut **tx).await?;
    sqlx::query("SET LOCAL ROLE board_migrator")
        .execute(&mut **tx)
        .await?;
    if active.is_some() && active != revision {
        return Err("Foreign active catalog must not change".into());
    }
    if command == "catalog-enable" {
        if revision.is_none() {
            let imported: i64 =
                sqlx::query_scalar("SELECT content.import_report_catalog($1::jsonb)")
                    .bind(SYNTHETIC_CATALOG)
                    .fetch_one(&mut **tx)
                    .await?;
            sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET catalog_revision=$2 WHERE marker=$1 AND catalog_revision IS NULL")
                .bind(&manifest.marker).bind(imported).execute(&mut **tx).await?;
            revision = Some(imported);
        }
        sqlx::query("SELECT content.set_report_catalog_active($1::bigint)")
            .bind(revision)
            .execute(&mut **tx)
            .await?;
        sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET catalog_groups=catalog_groups+1 WHERE marker=$1")
            .bind(&manifest.marker).execute(&mut **tx).await?;
    } else if active.is_some() {
        sqlx::query("SELECT content.set_report_catalog_active(NULL::bigint)")
            .execute(&mut **tx)
            .await?;
    }
    Ok(serde_json::json!({"revision":revision,"ruleId":9001,"illegalId":31}))
}

async fn remove_posting_actor(tx: &mut Transaction<'_, Postgres>, actor: &[u8; 32]) -> Result<()> {
    sqlx::query("DELETE FROM post_secrets.posting_history WHERE actor_hash=$1")
        .bind(actor.as_slice())
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM post_secrets.posting_thread_actions WHERE actor_hash=$1")
        .bind(actor.as_slice())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn remove_actor(tx: &mut Transaction<'_, Postgres>, actor: &[u8; 32]) -> Result<()> {
    let changed =
        sqlx::query("DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
            .bind(actor.as_slice())
            .execute(&mut **tx)
            .await?;
    if changed.rows_affected() > 1 {
        return Err("At most one owned actor may change".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The catalog test changes global admission mode; serialize database cases
    // even when the example test binary uses the default parallel test runner.
    static DATABASE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn manifest() -> Manifest {
        use rand_core::{OsRng, RngCore};
        fn token() -> String {
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
        Manifest {
            version: 1,
            key: token(),
            marker: token(),
            owner_pid: std::process::id(),
            database: "imageboard".into(),
        }
    }

    #[test]
    fn rejects_production_remote_wrong_role_database_and_sql_selectors() {
        let m = manifest();
        let url = "postgres://board_migrator:synthetic@127.0.0.1/imageboard";
        assert!(validate_target("development", url, &m).is_ok());
        assert!(validate_target("production", url, &m).is_err());
        for wrong in [
            "postgres://board_public:x@127.0.0.1/imageboard",
            "postgres://board_migrator:x@example.com/imageboard",
            "postgres://board_migrator:x@127.0.0.1/other",
            "postgres://board_migrator:x@127.0.0.1/imageboard?options=unsafe",
        ] {
            assert!(validate_target("development", wrong, &m).is_err());
        }
        let mut bad = manifest();
        bad.marker = "arbitrary actor or SQL selector".into();
        assert!(bad.validate().is_err());
        bad = manifest();
        bad.key = "0".repeat(64);
        assert!(bad.validate().is_err());
        assert_ne!(m.actor().unwrap(), manifest().actor().unwrap());
        assert_ne!(m.actor().unwrap(), m.posting_actor().unwrap());
        assert_eq!(m.posting_actor().unwrap(), m.posting_actor().unwrap());
    }

    #[test]
    fn synthetic_catalog_is_explicit_and_contains_only_fixed_test_rows() {
        let catalog: serde_json::Value = serde_json::from_str(SYNTHETIC_CATALOG).unwrap();
        assert_eq!(catalog["version"], 1);
        let rows = catalog["categories"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(row.as_object().unwrap().len(), 9);
        }
        assert_eq!(rows[0]["id"], 9001);
        assert_eq!(rows[0]["board"], "demo");
        assert_eq!(rows[0]["title"], "Synthetic board rule");
        assert_eq!(rows[0]["weight"], 1.25);
        assert_eq!(rows[1]["id"], 31);
        assert_eq!(rows[1]["title"], "Synthetic illegal content");
        assert_eq!(rows[1]["weight"], 2.5);
    }

    // Run serially with all other database tests in an exclusive disposable DB.
    #[tokio::test]
    async fn catalog_lease_is_lazy_exclusive_idempotent_and_never_clears_foreign_pointer() {
        let _database_guard = DATABASE_TEST_LOCK.lock().await;
        let cluster = std::env::var("BOARD_TEST_CLUSTER")
            .expect("Fresh disposable BOARD_TEST_CLUSTER required");
        let suffix = cluster
            .strip_prefix("/tmp/board-postgres.")
            .expect("Refusing non-disposable database cluster");
        assert!(
            suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
            "Refusing non-disposable database cluster"
        );
        let url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
        let pool = PgPool::connect(&url).await.unwrap();
        let database = url::Url::parse(&url)
            .unwrap()
            .path()
            .trim_start_matches('/')
            .to_owned();
        let mut own = manifest();
        own.database = database.clone();
        let mut foreign = manifest();
        foreign.database = database;
        validate_target(&std::env::var("APP_ENV").unwrap(), &url, &own).unwrap();
        let login: (String, String) =
            sqlx::query_as("SELECT session_user::text,current_user::text")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(login, ("board_migrator".into(), "board_migrator".into()));
        apply(&pool, &own, "init").await.unwrap();
        apply(&pool, &foreign, "init").await.unwrap();
        let exclusive_rejected = apply(&pool, &own, "catalog-enable").await.is_err();
        apply(&pool, &foreign, "finish").await.unwrap();
        // Retain the alternate revision before attempting activation, including
        // if its acknowledgement is lost or an assertion later unwinds.
        let mut temporary_revision = None;
        let outcome = async {
            assert!(exclusive_rejected);
            let revision: Option<i64> = sqlx::query_scalar("SELECT catalog_revision FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1")
                .bind(&own.marker).fetch_one(&pool).await.unwrap();
            assert_eq!(revision, None);
            apply(&pool, &own, "catalog-enable").await.unwrap();
            let revision: i64 = sqlx::query_scalar("SELECT catalog_revision FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1")
                .bind(&own.marker).fetch_one(&pool).await.unwrap();
            // Retry after an uncertain reply uses exactly the recorded import.
            apply(&pool, &own, "catalog-enable").await.unwrap();
            apply(&pool, &own, "catalog-disable").await.unwrap();
            apply(&pool, &own, "catalog-disable").await.unwrap();
            apply(&pool, &own, "catalog-enable").await.unwrap();
            let same: i64 = sqlx::query_scalar("SELECT catalog_revision FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1")
                .bind(&own.marker).fetch_one(&pool).await.unwrap();
            assert_eq!(same, revision);
            assert!(apply(&pool, &foreign, "init").await.is_err());
            assert!(apply(&pool, &foreign, "catalog-disable").await.is_err());
            assert!(
                apply_command(&pool, &own, "catalog-inspect", Some((1000001, 1000001)))
                    .await
                    .is_err()
            );
            let wrong = Manifest {
                key: manifest().key,
                marker: own.marker.clone(),
                database: own.database.clone(),
                ..manifest()
            };
            assert!(apply(&pool, &wrong, "catalog-disable").await.is_err());
            let other: i64 = sqlx::query_scalar("SELECT content.import_report_catalog($1::jsonb)")
                .bind(SYNTHETIC_CATALOG)
                .fetch_one(&pool)
                .await
                .unwrap();
            temporary_revision = Some(other);
            sqlx::query("SELECT content.set_report_catalog_active($1)")
                .bind(other)
                .execute(&pool)
                .await
                .unwrap();
            let enable_rejected = apply(&pool, &own, "catalog-enable").await.is_err();
            let disable_rejected = apply(&pool, &own, "catalog-disable").await.is_err();
            let finish_rejected = apply(&pool, &own, "finish").await.is_err();
            let mut check = pool.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_report_admission_owner")
                .execute(&mut *check)
                .await
                .unwrap();
            let preserved: Option<i64> = sqlx::query_scalar("SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton")
                .fetch_one(&mut *check).await.unwrap();
            check.rollback().await.unwrap();
            // Restore exactly the other revision imported by this test, before
            // checking assertions and retiring the owned lease.
            sqlx::query("SELECT content.set_report_catalog_active($1)")
                .bind(revision)
                .execute(&pool)
                .await
                .unwrap();
            assert!(enable_rejected && disable_rejected && finish_rejected);
            assert_eq!(preserved, Some(other));
            sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET catalog_groups=10000 WHERE marker=$1")
                .bind(&own.marker).execute(&pool).await.unwrap();
            assert!(apply(&pool, &own, "catalog-enable").await.is_err());
            revision
        };
        use futures_util::FutureExt;
        let checked = std::panic::AssertUnwindSafe(outcome).catch_unwind().await;
        // Cleanup survives panics while the deliberate foreign-pointer test is
        // active. Touch only its exact recorded revision under the normal gate;
        // an unrelated pointer remains untouched and keeps the lease proof.
        let restored: Result<()> = async {
            if let Some(temporary) = temporary_revision {
                let mut cleanup = pool.begin().await?;
                sqlx::query("SELECT pg_advisory_xact_lock(260085001)").execute(&mut *cleanup).await?;
                let owned_revision: Option<i64> = sqlx::query_scalar("SELECT catalog_revision FROM post_secrets.browser_deletion_fixture_runs WHERE marker=$1 AND actor_hash=$2 AND database_name=$3 AND posting_actor_hash=$4 AND expires_at>now() FOR UPDATE")
                    .bind(&own.marker).bind(own.actor()?.as_slice()).bind(&own.database)
                    .bind(own.posting_actor()?.as_slice()).fetch_one(&mut *cleanup).await?;
                sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *cleanup).await?;
                let active: Option<i64> = sqlx::query_scalar("SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
                    .fetch_one(&mut *cleanup).await?;
                sqlx::query("SET LOCAL ROLE board_migrator").execute(&mut *cleanup).await?;
                if active == Some(temporary) {
                    sqlx::query("SELECT content.set_report_catalog_active($1)")
                        .bind(owned_revision).execute(&mut *cleanup).await?;
                }
                cleanup.commit().await?;
            }
            Ok(())
        }.await;
        if restored.is_err() {
            pool.close().await;
            if let Err(panic) = checked {
                std::panic::resume_unwind(panic);
            }
            panic!("Exact catalog test restoration failed; ownership lease retained");
        }
        let finished = apply(&pool, &own, "finish").await;
        if finished.is_err() {
            pool.close().await;
            if let Err(panic) = checked {
                std::panic::resume_unwind(panic);
            }
            panic!("Exact catalog test retirement failed; ownership lease retained");
        }
        if let Ok(revision) = &checked {
            let retained: serde_json::Value =
                sqlx::query_scalar("SELECT content.read_report_catalog($1)")
                    .bind(*revision)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                retained,
                serde_json::from_str::<serde_json::Value>(SYNTHETIC_CATALOG).unwrap()
            );
            let mut check = pool.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_report_admission_owner")
                .execute(&mut *check)
                .await
                .unwrap();
            let active: Option<i64> = sqlx::query_scalar("SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton")
                .fetch_one(&mut *check).await.unwrap();
            check.rollback().await.unwrap();
            assert_eq!(active, None);
        }
        pool.close().await;
        if let Err(panic) = checked {
            std::panic::resume_unwind(panic);
        }
    }

    #[tokio::test]
    async fn exact_lease_rejects_wrong_marker_and_preserves_foreign_actor() {
        let _database_guard = DATABASE_TEST_LOCK.lock().await;
        let url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
        let pool = PgPool::connect(&url).await.unwrap();
        let database = url::Url::parse(&url)
            .unwrap()
            .path()
            .trim_start_matches('/')
            .to_owned();
        let mut own = manifest();
        own.database = database.clone();
        let mut foreign = manifest();
        foreign.database = database;
        // Both actors are fresh synthetic runs; no existing row is adopted.
        apply(&pool, &own, "init").await.unwrap();
        apply(&pool, &foreign, "init").await.unwrap();
        let outcome = async {
            // Simulate a lease made before the nullable posting-actor upgrade.
            // It cannot authorize any command or be adopted by a new init.
            sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET posting_actor_hash=NULL WHERE marker=$1 AND actor_hash=$2 AND posting_actor_hash=$3")
                .bind(&own.marker).bind(own.actor().unwrap().as_slice())
                .bind(own.posting_actor().unwrap().as_slice()).execute(&pool).await.unwrap();
            let mut legacy_rejections = Vec::new();
            for command in ["init", "reset", "reset-posting", "check", "finish"] {
                legacy_rejections.push(apply(&pool, &own, command).await.is_err());
            }
            // Restore only this test's exact proof before assertions or teardown.
            sqlx::query("UPDATE post_secrets.browser_deletion_fixture_runs SET posting_actor_hash=$3 WHERE marker=$1 AND actor_hash=$2 AND posting_actor_hash IS NULL")
                .bind(&own.marker).bind(own.actor().unwrap().as_slice())
                .bind(own.posting_actor().unwrap().as_slice()).execute(&pool).await.unwrap();
            assert!(legacy_rejections.into_iter().all(|rejected| rejected));
            for m in [&own, &foreign] {
                sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[1,2,3]::bigint[],9999999999)")
                    .bind(m.actor().unwrap().as_slice()).execute(&pool).await.unwrap();
            }
            let posts: Vec<(i64, i64)> = sqlx::query_as("SELECT id,thread_id FROM content.posts p WHERE board='demo' AND NOT EXISTS(SELECT 1 FROM post_secrets.posting_history h WHERE h.post_id=p.id) ORDER BY id LIMIT 2")
                .fetch_all(&pool).await.unwrap();
            assert_eq!(posts.len(), 2);
            for (m, (post, thread)) in [&own, &foreign].into_iter().zip(posts) {
                sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,'demo',$2,$3,123)")
                    .bind(post).bind(thread).bind(m.posting_actor().unwrap().as_slice()).execute(&pool).await.unwrap();
                sqlx::query("INSERT INTO post_secrets.posting_thread_actions(actor_hash,board,request_at) VALUES($1,'demo',123)")
                    .bind(m.posting_actor().unwrap().as_slice()).execute(&pool).await.unwrap();
            }
            assert!(apply(&pool, &own, "unknown").await.is_err());
            assert!(apply(&pool, &own, "init").await.is_err());
            let wrong = Manifest {
                marker: manifest().marker,
                ..manifest()
            };
            assert!(apply(&pool, &wrong, "reset").await.is_err());
            let wrong_marker = Manifest {
                marker: manifest().marker,
                key: own.key.clone(),
                database: own.database.clone(),
                ..manifest()
            };
            assert!(apply(&pool, &wrong_marker, "reset").await.is_err());
            assert!(apply(&pool, &wrong_marker, "reset-posting").await.is_err());
            assert!(apply(&pool, &wrong_marker, "finish").await.is_err());
            apply(&pool, &own, "reset-posting").await.unwrap();
            let own_history: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM post_secrets.posting_history WHERE actor_hash=$1",
            )
            .bind(own.posting_actor().unwrap().as_slice())
            .fetch_one(&pool)
            .await
            .unwrap();
            let own_actions: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM post_secrets.posting_thread_actions WHERE actor_hash=$1",
            )
            .bind(own.posting_actor().unwrap().as_slice())
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!((own_history, own_actions), (0, 0));
            let foreign_history: Vec<i64> = sqlx::query_scalar(
                "SELECT request_at FROM post_secrets.posting_history WHERE actor_hash=$1",
            )
            .bind(foreign.posting_actor().unwrap().as_slice())
            .fetch_all(&pool)
            .await
            .unwrap();
            let foreign_actions: Vec<i64> = sqlx::query_scalar(
                "SELECT request_at FROM post_secrets.posting_thread_actions WHERE actor_hash=$1",
            )
            .bind(foreign.posting_actor().unwrap().as_slice())
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(foreign_history, vec![123]);
            assert_eq!(foreign_actions, vec![123]);
            let preserved_deletion: Vec<i64> = sqlx::query_scalar(
                "SELECT events FROM post_secrets.public_deletion_actors WHERE actor_hash=$1",
            )
            .bind(own.actor().unwrap().as_slice())
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(preserved_deletion, vec![1, 2, 3]);
            assert!(apply(&pool, &own, "check").await.is_err());
            apply(&pool, &own, "reset").await.unwrap();
            apply(&pool, &own, "check").await.unwrap();
            sqlx::query("INSERT INTO post_secrets.public_deletion_actors(actor_hash,events,expires_at) VALUES($1,ARRAY[1,2,3,4]::bigint[],9999999999)")
                .bind(own.actor().unwrap().as_slice()).execute(&pool).await.unwrap();
            assert!(apply(&pool, &own, "check").await.is_err());
            apply(&pool, &own, "reset-posting").await.unwrap();
            assert!(apply(&pool, &own, "check").await.is_err());
            let unchanged: (Vec<i64>, i64) = sqlx::query_as("SELECT events,expires_at FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
                .bind(foreign.actor().unwrap().as_slice()).fetch_one(&pool).await.unwrap();
            assert_eq!(unchanged, (vec![1, 2, 3], 9999999999));
        };
        // Always retire the two exact synthetic runs, even if an assertion fails.
        use futures_util::FutureExt;
        let checked = std::panic::AssertUnwindSafe(outcome).catch_unwind().await;
        apply(&pool, &own, "finish").await.unwrap();
        // Capture foreign state after own teardown, but always retire the foreign
        // lease before unwrapping or asserting any observation.
        let foreign_after_finish: std::result::Result<i64, sqlx::Error> = sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_thread_actions WHERE actor_hash=$1 AND request_at=123")
            .bind(foreign.posting_actor().unwrap().as_slice()).fetch_one(&pool).await;
        apply(&pool, &foreign, "finish").await.unwrap();
        if checked.is_ok() {
            assert_eq!(foreign_after_finish.unwrap(), 1);
        }
        assert!(apply(&pool, &own, "reset").await.is_err());
        assert!(apply(&pool, &own, "reset-posting").await.is_err());
        pool.close().await;
        if let Err(panic) = checked {
            std::panic::resume_unwind(panic);
        }
    }
}
