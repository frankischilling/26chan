//! Explicit, bounded cleanup of payload already marked deleted. This operator
//! path has no runtime caller and never deletes structural media tombstones.
use board_domain::BoardSlug;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

const LIMIT: i64 = 64;
const POST_GUARD: &str = "71cacbdcdcc037cd3628d0f06d742aab41c08aded68bc2728c0e236af1ce887c";
const THREAD_GUARD: &str = "01aff94037bf0af55a594fe13916bbe6255bcf2925d5b7f1bbfe91aaa977b63b";

#[derive(Debug, thiserror::Error)]
pub enum ErasureError {
    #[error("A canonical board and a lowercase 64-digit preview receipt are required.")]
    Input,
    #[error("Historical cleanup requires the actual board_migrator login.")]
    Role,
    #[error("Historical cleanup schema or visibility is unavailable.")]
    Schema,
    #[error("The board is unavailable.")]
    Board,
    #[error("The preview changed. Nothing was erased; obtain and review a new preview.")]
    Stale,
    #[error("Cannot confirm historical cleanup. Recheck database state before retrying.")]
    Database,
}

#[derive(Serialize)]
struct Snapshot {
    version: u8,
    database: String,
    board: String,
    reports: Vec<(i64, String)>,
    posts: Vec<(i64, i64, String)>,
    threads: Vec<(i64, String)>,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub database: String,
    pub board: String,
    pub report_ids: Vec<i64>,
    pub post_ids: Vec<i64>,
    pub thread_ids: Vec<i64>,
    pub receipt: String,
    pub applied: bool,
}

impl Snapshot {
    fn preview(&self, applied: bool) -> Result<Preview, ErasureError> {
        let bytes = serde_json::to_vec(self).map_err(|_| ErasureError::Database)?;
        let receipt = format!("{:x}", Sha256::digest(bytes));
        Ok(Preview {
            database: self.database.clone(),
            board: self.board.clone(),
            report_ids: self.reports.iter().map(|row| row.0).collect(),
            post_ids: self.posts.iter().map(|row| row.0).collect(),
            thread_ids: self.threads.iter().map(|row| row.0).collect(),
            receipt,
            applied,
        })
    }
}

/// Preview rolls back. Apply recomputes the same bounded selection under the
/// board lock and requires its reviewed receipt. It never loops over batches.
/// The operator must stop writers and schema changes for the cleanup window.
pub async fn cleanup(
    connection: &mut PgConnection,
    board: &str,
    receipt: Option<&str>,
) -> Result<Preview, ErasureError> {
    BoardSlug::parse(board).map_err(|_| ErasureError::Input)?;
    if receipt.is_some_and(|value| {
        value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(ErasureError::Input);
    }
    let mut tx = connection
        .begin()
        .await
        .map_err(|_| ErasureError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .map_err(|_| ErasureError::Database)?;
    let operator: bool = sqlx::query_scalar(
        "SELECT session_user='board_migrator' AND current_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ErasureError::Database)?;
    if !operator {
        return Err(ErasureError::Role);
    }
    sqlx::raw_sql("SET LOCAL statement_timeout='10s'; SET LOCAL lock_timeout='2s'; SET LOCAL idle_in_transaction_session_timeout='10s'; SET LOCAL search_path=pg_catalog; SET LOCAL TimeZone='UTC'; SET LOCAL DateStyle='ISO, YMD'")
        .execute(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    preflight(&mut tx).await?;
    let exists: Option<String> =
        sqlx::query_scalar("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(board)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ErasureError::Database)?;
    if exists.is_none() {
        return Err(ErasureError::Board);
    }
    let database = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ErasureError::Database)?;
    // Full-row digests stay inside the private snapshot. The public command
    // output includes only IDs and one receipt, never content or author links.
    let reports = sqlx::query_as("SELECT r.id,encode(sha256(convert_to(to_jsonb(r)::text,'UTF8')),'hex') FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE r.board=$1 AND (p.deleted OR t.deleted) ORDER BY r.id LIMIT $2 FOR UPDATE OF r,p,t")
        .bind(board).bind(LIMIT).fetch_all(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    // Reports are drained before their post. A later invocation picks up a
    // newly report-free post; a large report group cannot enlarge this batch.
    let posts = sqlx::query_as("SELECT p.id,p.thread_id,encode(sha256(convert_to(to_jsonb(p)::text,'UTF8')),'hex') FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=$1 AND (p.deleted OR t.deleted) AND NOT p.content_erased AND NOT EXISTS(SELECT 1 FROM content.reports r WHERE r.board=p.board AND r.post_id=p.id) ORDER BY p.thread_id,p.id LIMIT $2 FOR UPDATE OF p,t")
        .bind(board).bind(LIMIT).fetch_all(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    // Parent metadata is last. Its ordinary descendant trigger has zero work.
    let threads = sqlx::query_as("SELECT t.id,encode(sha256(convert_to(to_jsonb(t)::text,'UTF8')),'hex') FROM content.threads t WHERE t.board=$1 AND t.deleted AND NOT t.content_erased AND NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.board=t.board AND p.thread_id=t.id AND NOT p.content_erased) AND NOT EXISTS(SELECT 1 FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board WHERE p.board=t.board AND p.thread_id=t.id) ORDER BY t.id LIMIT $2 FOR UPDATE OF t")
        .bind(board).bind(LIMIT).fetch_all(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    let snapshot = Snapshot {
        version: 1,
        database,
        board: board.to_owned(),
        reports,
        posts,
        threads,
    };
    let preview = snapshot.preview(receipt.is_some())?;
    if let Some(receipt) = receipt {
        if receipt != preview.receipt {
            return Err(ErasureError::Stale);
        }
        let removed = sqlx::query("DELETE FROM content.reports WHERE board=$1 AND id=ANY($2)")
            .bind(board)
            .bind(&preview.report_ids)
            .execute(&mut *tx)
            .await
            .map_err(|_| ErasureError::Database)?
            .rows_affected();
        let posts = sqlx::query("UPDATE content.posts SET deleted=true,content_erased=true WHERE board=$1 AND id=ANY($2) AND NOT content_erased")
            .bind(board).bind(&preview.post_ids).execute(&mut *tx).await.map_err(|_| ErasureError::Database)?.rows_affected();
        let threads = sqlx::query("UPDATE content.threads SET deleted=true,content_erased=true WHERE board=$1 AND id=ANY($2) AND deleted AND NOT content_erased")
            .bind(board).bind(&preview.thread_ids).execute(&mut *tx).await.map_err(|_| ErasureError::Database)?.rows_affected();
        if removed != preview.report_ids.len() as u64
            || posts != preview.post_ids.len() as u64
            || threads != preview.thread_ids.len() as u64
        {
            return Err(ErasureError::Stale);
        }
        preflight(&mut tx).await?;
        tx.commit().await.map_err(|_| ErasureError::Database)?;
    } else {
        tx.rollback().await.map_err(|_| ErasureError::Database)?;
    }
    Ok(preview)
}

pub(crate) async fn preflight(connection: &mut PgConnection) -> Result<(), ErasureError> {
    // Do not let a later schema add an unreviewed cascade to this operator.
    // The existing report/snapshot checks below verify each expected FK too.
    let cascades: bool = sqlx::query_scalar("SELECT (SELECT count(*)=4 AND bool_and(k.confdeltype='c' AND k.convalidated AND k.conrelid IN ('post_secrets.report_membership'::regclass,'post_secrets.anonymous_reports'::regclass,'post_secrets.report_weight_evidence'::regclass,'post_secrets.report_post_snapshots'::regclass)) FROM pg_constraint k WHERE k.contype='f' AND k.confrelid='content.reports'::regclass) AND EXISTS(SELECT 1 FROM pg_class WHERE oid='content.reports'::regclass AND relowner='board_migrator'::regrole AND relkind='r' AND relpersistence='p' AND NOT relforcerowsecurity)")
        .fetch_one(&mut *connection).await.map_err(|_| ErasureError::Schema)?;
    if !cascades {
        return Err(ErasureError::Schema);
    }
    let guards: bool = sqlx::query_scalar("SELECT count(*)=2 AND bool_and(p.proowner='board_content_erasure_owner'::regrole AND p.prosecdef AND p.prorettype='trigger'::regtype AND p.pronargs=0 AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[] AND encode(sha256(convert_to(p.prosrc,'UTF8')),'hex')=v.body) FROM (VALUES('post_secrets.guard_post_erasure()',$1::text),('post_secrets.guard_thread_erasure()',$2::text)) v(signature,body) JOIN pg_proc p ON p.oid=to_regprocedure(v.signature)")
        .bind(POST_GUARD).bind(THREAD_GUARD).fetch_one(&mut *connection).await.map_err(|_| ErasureError::Schema)?;
    if !guards {
        return Err(ErasureError::Schema);
    }
    // The existing seal covers content/authority trigger definitions and ACLs.
    // Assume only the already-granted operator owner, never a runtime identity.
    sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    let erasure: bool = sqlx::query_scalar(crate::content_erasure::READINESS_SQL)
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    sqlx::query("RESET ROLE")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    if !ready || !erasure {
        return Err(ErasureError::Schema);
    }
    // The report-profile seal is granted to the real migrator, not the
    // restriction owner. Check it before changing role, then pass that result
    // into the shared query. Neither owner needs the other's private grants.
    let profile: bool = sqlx::query_scalar("SELECT content.report_profile_ready()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    if !profile {
        return Err(ErasureError::Schema);
    }
    // Inspect configured public permissions without granting or assuming the
    // public runtime role. The report check also calls restrictions_ready();
    // the migrator has SET-only membership in its owner, not direct EXECUTE.
    // Reuse that existing owner solely for this sealed readiness check.
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    // No SQL or identity text comes from the command line.
    let report_catalog = crate::report_admission::READINESS_SQL
        .replace("current_user", "'board_public'::name")
        .replace("content.report_profile_ready()", "$1::boolean");
    let reports: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(report_catalog))
        .bind(profile)
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    sqlx::query("RESET ROLE")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    if !reports {
        return Err(ErasureError::Schema);
    }
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    let snapshots: bool = sqlx::query_scalar("SELECT content.report_snapshots_ready()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    sqlx::query("RESET ROLE")
        .execute(&mut *connection)
        .await
        .map_err(|_| ErasureError::Schema)?;
    if !snapshots {
        return Err(ErasureError::Schema);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_guard_pins_match_the_latest_migration_bodies() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
        let mut files: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
            .collect();
        files.sort();
        for (name, expected) in [
            ("guard_post_erasure", POST_GUARD),
            ("guard_thread_erasure", THREAD_GUARD),
        ] {
            let marker = format!("FUNCTION post_secrets.{name}() RETURNS trigger");
            let mut latest = None;
            for path in &files {
                let text = std::fs::read_to_string(path).unwrap();
                if let Some((_, definition)) = text.split_once(&marker) {
                    let body = definition
                        .split_once("AS $$")
                        .unwrap()
                        .1
                        .split_once("$$;")
                        .unwrap()
                        .0;
                    latest = Some(format!("{:x}", Sha256::digest(body.as_bytes())));
                }
            }
            assert_eq!(
                latest.as_deref(),
                Some(expected),
                "Operator pin differs from latest {name}"
            );
        }
    }

    #[test]
    fn receipt_binds_scope_order_and_saved_state_without_exposing_payload() {
        let mut snapshot = Snapshot {
            version: 1,
            database: "owned".into(),
            board: "test".into(),
            reports: vec![(1, "private-row-digest".into())],
            posts: vec![],
            threads: vec![],
        };
        let first = snapshot.preview(false).unwrap();
        assert!(
            !serde_json::to_string(&first)
                .unwrap()
                .contains("private-row-digest")
        );
        assert_eq!(first.receipt.len(), 64);
        snapshot.board = "other".into();
        assert_ne!(first.receipt, snapshot.preview(false).unwrap().receipt);
        snapshot.board = "test".into();
        snapshot.reports[0].1 = "changed".into();
        assert_ne!(first.receipt, snapshot.preview(false).unwrap().receipt);
    }
}
