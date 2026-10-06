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

/// Include navigation in the same transaction as the HTML archive listing.
pub async fn archive_page_snapshot(
    pool: &PgPool,
    slug: &str,
) -> Result<PageSnapshot<ArchiveSnapshot>, StoreError> {
    read_archive_snapshot(pool, slug, true).await
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
    // Source HTML includes up to 3,000 root clocks from the last 72 hours, newest first.
    // The transaction clock keeps that window consistent with this read snapshot.
    // archive.json lists all retained entries by ID.
    // Equal bump clocks use descending IDs as a deterministic local tie-break.
    let query = if include_navigation {
        "SELECT t.id,p.subject,t.archived_at FROM content.visible_threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted AND t.bumped_at>=transaction_timestamp()-interval '72 hours' ORDER BY t.bumped_at DESC,t.id DESC LIMIT 3000"
    } else {
        "SELECT t.id,p.subject,t.archived_at FROM content.visible_threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id WHERE t.board=$1 AND t.archived_at IS NOT NULL AND NOT p.deleted ORDER BY t.id LIMIT 1000"
    };
    let entries = sqlx::query_as(query).bind(slug).fetch_all(&mut *tx).await?;
    let navigation_boards = crate::read::snapshot_navigation(&mut tx, include_navigation).await?;
    tx.commit().await?;
    Ok(PageSnapshot {
        snapshot: ArchiveSnapshot { board, entries },
        navigation_boards,
    })
}

/// Caller holds the board row lock through the new OP's commit.
pub(crate) async fn make_room(
    tx: &mut Transaction<'_, Postgres>,
    board: &Board,
) -> Result<(), StoreError> {
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
        let victims: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL AND NOT sticky AND NOT undead ORDER BY bumped_at,id LIMIT $2 FOR UPDATE")
            .bind(&board.slug).bind(needed).fetch_all(&mut **tx).await?;
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
    // Keep the published archive response bounded, including when policy shrinks.
    sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND NOT deleted AND archived_at IS NOT NULL AND (archive_expires_at<=statement_timestamp() OR $3=0 OR id IN (SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NOT NULL AND archive_expires_at>statement_timestamp() ORDER BY archived_at DESC,id DESC OFFSET $2))")
        .bind(&board.slug).bind(i64::from(board.archive_limit)).bind(board.archive_retention_seconds).execute(&mut **tx).await?;
    Ok(())
}
