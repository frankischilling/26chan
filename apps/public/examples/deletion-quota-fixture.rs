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
    if args.len() != 1
        || !matches!(
            args[0].as_str(),
            "init" | "reset" | "reset-posting" | "check" | "finish"
        )
    {
        return Err("One fixed fixture command required".into());
    }
    let manifest = read_manifest(Path::new(&std::env::var(
        "BROWSER_DELETION_QUOTA_MANIFEST",
    )?))?;
    let database_url = std::env::var("MIGRATION_DATABASE_URL")?;
    validate_target(&std::env::var("APP_ENV")?, &database_url, &manifest)?;
    let pool = PgPool::connect(&database_url).await?;
    let result = apply(&pool, &manifest, &args[0]).await;
    pool.close().await;
    result
}

async fn apply(pool: &PgPool, manifest: &Manifest, command: &str) -> Result<()> {
    manifest.validate()?;
    if !matches!(
        command,
        "init" | "reset" | "reset-posting" | "check" | "finish"
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
    // Serialize only fixture initialization DDL, not arbitrary application work.
    if command == "init" {
        sqlx::query("SELECT pg_advisory_xact_lock(260085001)")
            .execute(&mut *tx)
            .await?;
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
        sqlx::query("ALTER TABLE post_secrets.browser_deletion_fixture_runs ADD COLUMN IF NOT EXISTS posting_actor_hash bytea UNIQUE CHECK(octet_length(posting_actor_hash)=32), ADD COLUMN IF NOT EXISTS posting_groups integer NOT NULL DEFAULT 0 CHECK(posting_groups BETWEEN 0 AND 10000)")
            .execute(&mut *tx).await?;
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
            "finish" => {
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
    Ok(())
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

    #[tokio::test]
    async fn exact_lease_rejects_wrong_marker_and_preserves_foreign_actor() {
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
