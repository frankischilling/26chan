use crate::{Board, PageSnapshot, PgPool, StoreError};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
pub struct ArchiveEntry {
    pub id: i64,
    pub subject: String,
    pub archived_at: DateTime<Utc>,
}

pub struct ArchiveSnapshot {
    pub board: Board,
    pub entries: Vec<ArchiveEntry>,
}

pub async fn archive_snapshot(pool: &PgPool, slug: &str) -> Result<ArchiveSnapshot, StoreError> {
    Ok(read_archive_snapshot(pool, slug, false).await?.snapshot)
}

/// HTML includes saved formatting inputs; JSON deliberately remains metadata-only.
#[derive(sqlx::FromRow)]
pub struct ArchivePageEntry {
    pub id: i64,
    pub subject: String,
    pub archived_at: DateTime<Utc>,
    pub comment: String,
    pub drawing_time_seconds: Option<i32>,
    pub drawing_source_post_id: Option<i64>,
    pub comment_format: i16,
    pub staff_authorized_limits: bool,
    pub wordfilter_payload: Option<Vec<u8>>,
    pub saved_presentation: Option<Vec<u8>>,
    pub dice_result: Option<String>,
    pub fortune_text: Option<String>,
    pub fortune_color: Option<String>,
}

pub struct ArchivePageSnapshot {
    pub board: Board,
    pub entries: Vec<ArchivePageEntry>,
}

/// Fail closed before loading potentially large saved comments/filter payloads.
pub const MAX_ARCHIVE_PAGE_READ_BYTES: usize = 8 * 1024 * 1024;
/// Refuse an oversized complete JSON listing instead of returning partial IDs.
pub const MAX_ARCHIVE_JSON_ENTRIES: usize = 100_000;

/// Include navigation and the comment-byte preflight in the same transaction.
pub async fn archive_page_snapshot(
    pool: &PgPool,
    slug: &str,
) -> Result<PageSnapshot<ArchivePageSnapshot>, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board: Board = sqlx::query_as(
        "SELECT * FROM content.boards WHERE slug=$1 AND archive_retention_seconds<>0",
    )
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    // Apply the source HTML window and 3,000-row ceiling before fetching bodies.
    // JSON has a separate complete-list contract.
    // The lateral primary-key lookup prevents stale board statistics from
    // reversing the join into one full thread scan for every post. LIMIT 1
    // preserves the unique-ID join and keeps the post RLS policy in force.
    let bytes: i64 = sqlx::query_scalar("SELECT coalesce(sum(body_bytes),0)::bigint FROM (SELECT octet_length(p.subject)::bigint+octet_length(p.comment)+coalesce(octet_length(p.wordfilter_payload),0)+coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)+coalesce(octet_length(p.saved_presentation_search),0)+coalesce(octet_length(p.dice_result),0)+coalesce(octet_length(p.fortune_text),0)+coalesce(octet_length(p.fortune_color),0) AS body_bytes FROM content.visible_threads t JOIN LATERAL (SELECT p.* FROM content.posts p WHERE p.board=t.board AND p.id=t.id LIMIT 1) p ON true WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted AND t.bumped_at>=transaction_timestamp()-interval '72 hours' ORDER BY t.bumped_at DESC,t.id DESC LIMIT 3000) selected")
        .bind(slug).fetch_one(&mut *tx).await?;
    if bytes > MAX_ARCHIVE_PAGE_READ_BYTES as i64 {
        return Err(StoreError::ReadLimit);
    }
    let entries = sqlx::query_as("SELECT t.id,p.subject,t.archived_at,p.comment,p.drawing_time_seconds,p.drawing_source_post_id,p.comment_format,p.staff_authorized_limits,p.wordfilter_payload,p.saved_presentation,p.dice_result,p.fortune_text,p.fortune_color FROM content.visible_threads t JOIN LATERAL (SELECT p.* FROM content.posts p WHERE p.board=t.board AND p.id=t.id LIMIT 1) p ON true WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted AND t.bumped_at>=transaction_timestamp()-interval '72 hours' ORDER BY t.bumped_at DESC,t.id DESC LIMIT 3000")
        .bind(slug).fetch_all(&mut *tx).await?;
    let navigation_boards = crate::read::snapshot_navigation(&mut tx, true).await?;
    tx.commit().await?;
    Ok(PageSnapshot {
        snapshot: ArchivePageSnapshot { board, entries },
        navigation_boards,
        blotter: Vec::new(),
    })
}

async fn read_archive_snapshot(
    pool: &PgPool,
    slug: &str,
    include_navigation: bool,
) -> Result<PageSnapshot<ArchiveSnapshot>, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board: Board = sqlx::query_as(
        "SELECT * FROM content.boards WHERE slug=$1 AND archive_retention_seconds<>0",
    )
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    // JSON lists every retained archive ID, independently of the HTML window.
    // Count one beyond the finite reader budget, then reject the entire response.
    // Use the same per-thread post lookup as HTML even when statistics predict
    // just one row for a newly populated board.
    let (count, bytes): (i64, i64) = sqlx::query_as("SELECT count(*),coalesce(sum(octet_length(subject)),0)::bigint FROM (SELECT p.subject FROM content.visible_threads t JOIN LATERAL (SELECT p.* FROM content.posts p WHERE p.board=t.board AND p.id=t.id LIMIT 1) p ON true WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted ORDER BY t.id LIMIT $2) selected")
        .bind(slug).bind(MAX_ARCHIVE_JSON_ENTRIES as i64 + 1).fetch_one(&mut *tx).await?;
    if count > MAX_ARCHIVE_JSON_ENTRIES as i64 || bytes > MAX_ARCHIVE_PAGE_READ_BYTES as i64 {
        return Err(StoreError::ReadLimit);
    }
    let entries = sqlx::query_as("SELECT t.id,p.subject,t.archived_at FROM content.visible_threads t JOIN LATERAL (SELECT p.* FROM content.posts p WHERE p.board=t.board AND p.id=t.id LIMIT 1) p ON true WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted ORDER BY t.id LIMIT $2")
        .bind(slug).bind(MAX_ARCHIVE_JSON_ENTRIES as i64).fetch_all(&mut *tx).await?;
    let navigation_boards = crate::read::snapshot_navigation(&mut tx, include_navigation).await?;
    tx.commit().await?;
    Ok(PageSnapshot {
        snapshot: ArchiveSnapshot { board, entries },
        navigation_boards,
        blotter: Vec::new(),
    })
}

/// Caller holds the board row lock through the new OP's commit.
pub(crate) async fn make_room(
    tx: &mut Transaction<'_, Postgres>,
    board: &Board,
) -> Result<(), StoreError> {
    // Source trim_db skips active rollover for JANITOR_BOARD, imported as
    // staff_only. This is board policy, independent of the posting actor.
    if !board.staff_only {
        // Source capacity applies only to ordinary OPs; protected threads neither
        // consume ordinary slots nor qualify as rollover victims.
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL AND NOT sticky AND NOT undead")
            .bind(&board.slug).fetch_one(&mut **tx).await?;
        let needed = count - i64::from(board.thread_limit) + 1;
        if needed > 0 {
            if needed > 1000 {
                return Err(StoreError::Conflict(
                    "Board capacity requires operator reconciliation.",
                ));
            }
            // Source EXPIRE_NEGLECTED switches root/bump-clock ordering to OP ID.
            // The caller loaded this policy while acquiring the board row lock, so
            // queued mutations see an operator change committed ahead of them.
            // Equal bump clocks use IDs as a deterministic local tie-break.
            let victim_query = if board.expire_neglected {
                "SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL AND NOT sticky AND NOT undead ORDER BY bumped_at,id LIMIT $2 FOR UPDATE"
            } else {
                "SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL AND NOT sticky AND NOT undead ORDER BY id LIMIT $2 FOR UPDATE"
            };
            let victims: Vec<i64> = sqlx::query_scalar(victim_query)
                .bind(&board.slug)
                .bind(needed)
                .fetch_all(&mut **tx)
                .await?;
            if victims.len() as i64 != needed {
                return Err(StoreError::Conflict(
                    "Board capacity requires operator reconciliation.",
                ));
            }
            if board.archives_enabled() {
                sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=CASE WHEN $3::integer=-1 THEN 'infinity'::timestamptz ELSE statement_timestamp()+make_interval(secs => $3::double precision) END,bumped_at=statement_timestamp() WHERE board=$1 AND id=ANY($2)")
                    .bind(&board.slug).bind(&victims).bind(board.archive_retention_seconds).execute(&mut **tx).await?;
            } else {
                sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=ANY($2)")
                    .bind(&board.slug).bind(&victims).execute(&mut **tx).await?;
            }
        }
    }
    // The source has age-based storage retention, not an archive-count cap.
    // Disabled archiving does not retroactively delete stored archives.
    if board.archive_retention_seconds > 0 {
        let cutoff = sqlx::query_scalar("SELECT statement_timestamp()")
            .fetch_one(&mut **tx)
            .await?;
        retire_archives_before(tx, &board.slug, cutoff).await?;
    }
    Ok(())
}

// Source trim_archive uses root < NOW() - age. Keep equality stored; the
// public view independently enforces its stricter read-time privacy deadline.
// The caller owns the board lock and captures the database cutoff after waiting.
async fn retire_archives_before(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    cutoff: DateTime<Utc>,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND NOT deleted AND archived_at IS NOT NULL AND archive_expires_at<$2")
        .bind(board).bind(cutoff).execute(&mut **tx).await?;
    Ok(())
}

/// Caller holds the board and parent locks and has completed reply admission.
/// Count live replies rather than the lifetime counter, matching source
/// count_thread_replies after deleted rows have left the original table.
pub(crate) async fn autoarchive_after_reply(
    tx: &mut Transaction<'_, Postgres>,
    board: &Board,
    parent: i64,
) -> Result<(), StoreError> {
    if board.autoarchive_cap == 0 || board.archive_retention_seconds == 0 {
        return Ok(());
    }
    let state: Option<(bool, bool, i64)> = sqlx::query_as("SELECT t.sticky,t.undead,(SELECT count(*) FROM content.posts p WHERE p.board=t.board AND p.thread_id=t.id AND p.id<>t.id AND NOT p.deleted) FROM content.threads t WHERE t.board=$1 AND t.id=$2 AND NOT t.deleted AND t.archived_at IS NULL")
        .bind(&board.slug).bind(parent).fetch_optional(&mut **tx).await?;
    let Some((sticky, undead, replies)) = state else {
        return Ok(());
    };
    if !should_autoarchive(board.autoarchive_cap, true, sticky, undead, true, replies) {
        return Ok(());
    }
    sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=CASE WHEN $3::integer=-1 THEN 'infinity'::timestamptz ELSE statement_timestamp()+make_interval(secs => $3::double precision) END,bumped_at=statement_timestamp() WHERE board=$1 AND id=$2")
        .bind(&board.slug).bind(parent).bind(board.archive_retention_seconds)
        .execute(&mut **tx).await?;
    Ok(())
}

fn should_autoarchive(
    cap: i32,
    reply: bool,
    sticky: bool,
    undead: bool,
    enabled: bool,
    count: i64,
) -> bool {
    cap > 0 && reply && !sticky && !undead && enabled && count >= i64::from(cap)
}

#[cfg(test)]
mod autoarchive_tests {
    #[test]
    fn source_decision_vectors() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/autoarchive-reference.json"))
                .unwrap();
        for row in reference["cases"].as_array().unwrap() {
            assert_eq!(
                super::should_autoarchive(
                    row["cap"].as_i64().unwrap() as i32,
                    row["reply"].as_bool().unwrap(),
                    row["sticky"].as_bool().unwrap(),
                    row["undead"].as_bool().unwrap(),
                    row["enabled"].as_bool().unwrap(),
                    row["live_replies"].as_i64().unwrap()
                ),
                row["archive"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod expiry_boundary_tests {
    use super::*;

    #[tokio::test]
    async fn archive_expiry_boundary_preserves_equality_but_hides_it_from_public_reads() {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let public = crate::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let board: String =
            sqlx::query_scalar("SELECT 'ab'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES($1,'Owned expiry boundary','',1000,100,100,100,10,86400)")
            .bind(&board).execute(&owner).await.unwrap();
        let (o, p, b) = (owner.clone(), public.clone(), board.clone());
        let result =
            tokio::spawn(async move {
                let mut tx = p.begin().await.unwrap();
                let cutoff: DateTime<Utc> = sqlx::query_scalar("SELECT transaction_timestamp()")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                let mut ids = Vec::new();
                for offset in [-1_i64, 0, 1] {
                    let id: i64 = sqlx::query_scalar(
                        "WITH inserted AS (
                       INSERT INTO content.threads(board,archived_at,archive_expires_at)
                       VALUES($1,$2::timestamptz-interval '1 day',$3) RETURNING id
                     )
                     INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
                     SELECT id,$1,id,'Anonymous','Owned boundary','Synthetic expiry boundary'
                     FROM inserted RETURNING id",
                    )
                    .bind(&b)
                    .bind(cutoff)
                    .bind(cutoff + chrono::Duration::microseconds(offset))
                    .fetch_one(&o)
                    .await
                    .unwrap();
                    ids.push(id);
                }
                sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                    .bind(&b)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                let visible: Vec<i64> = sqlx::query_scalar(
                    "SELECT id FROM content.visible_threads WHERE board=$1 ORDER BY id",
                )
                .bind(&b)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
                assert_eq!(
                    visible,
                    vec![ids[2]],
                    "read-time privacy excludes equality and expired entries"
                );
                retire_archives_before(&mut tx, &b, cutoff).await.unwrap();
                tx.commit().await.unwrap();
                let states: Vec<(i64, bool, bool)> = sqlx::query_as(
                "SELECT id,deleted,content_erased FROM content.threads WHERE board=$1 ORDER BY id"
            ).bind(&b).fetch_all(&o).await.unwrap();
                assert_eq!(
                    states,
                    vec![
                        (ids[0], true, true),
                        (ids[1], false, false),
                        (ids[2], false, false)
                    ],
                    "only strictly expired storage is erased; equality survives this cleanup"
                );
            })
            .await;
        for query in [
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(query)
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
        }
        public.close().await;
        owner.close().await;
        result.unwrap();
    }
}
