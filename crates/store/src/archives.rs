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
        "SELECT * FROM content.boards WHERE slug=$1 AND archive_retention_seconds>0",
    )
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    // Same 72-hour transaction clock, 3,000-row ceiling and deterministic
    // bump-clock/ID ordering as the metadata listing, before fetching bodies.
    let bytes: i64 = sqlx::query_scalar("SELECT coalesce(sum(body_bytes),0)::bigint FROM (SELECT octet_length(p.subject)::bigint+octet_length(p.comment)+coalesce(octet_length(p.wordfilter_payload),0)+coalesce(octet_length(p.dice_result),0)+coalesce(octet_length(p.fortune_text),0)+coalesce(octet_length(p.fortune_color),0) AS body_bytes FROM content.visible_threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted AND t.bumped_at>=transaction_timestamp()-interval '72 hours' ORDER BY t.bumped_at DESC,t.id DESC LIMIT 3000) selected")
        .bind(slug).fetch_one(&mut *tx).await?;
    if bytes > MAX_ARCHIVE_PAGE_READ_BYTES as i64 {
        return Err(StoreError::ReadLimit);
    }
    let entries = sqlx::query_as("SELECT t.id,p.subject,t.archived_at,p.comment,p.drawing_time_seconds,p.drawing_source_post_id,p.comment_format,p.staff_authorized_limits,p.wordfilter_payload,p.dice_result,p.fortune_text,p.fortune_color FROM content.visible_threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted AND t.bumped_at>=transaction_timestamp()-interval '72 hours' ORDER BY t.bumped_at DESC,t.id DESC LIMIT 3000")
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
        "SELECT * FROM content.boards WHERE slug=$1 AND archive_retention_seconds>0",
    )
    .bind(slug)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    let entries = sqlx::query_as("SELECT t.id,p.subject,t.archived_at FROM content.visible_threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted ORDER BY t.id LIMIT 1000")
        .bind(slug).fetch_all(&mut *tx).await?;
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
            if board.archive_retention_seconds > 0 {
                sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=statement_timestamp()+make_interval(secs => $3::double precision),modified_at=clock_timestamp() WHERE board=$1 AND id=ANY($2)")
                    .bind(&board.slug).bind(&victims).bind(board.archive_retention_seconds).execute(&mut **tx).await?;
            } else {
                sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=ANY($2)")
                    .bind(&board.slug).bind(&victims).execute(&mut **tx).await?;
            }
        }
    }
    // Source trim_archive is separate and has no private-board exemption.
    // Keep the published archive response bounded, including when policy shrinks.
    sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND NOT deleted AND archived_at IS NOT NULL AND (archive_expires_at<=statement_timestamp() OR $3=0 OR id IN (SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NOT NULL AND archive_expires_at>statement_timestamp() ORDER BY archived_at DESC,id DESC OFFSET $2))")
        .bind(&board.slug).bind(i64::from(board.archive_limit)).bind(board.archive_retention_seconds).execute(&mut **tx).await?;
    Ok(())
}
