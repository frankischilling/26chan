//! Bounded, unweighted inventory of uncleared retained reports.
//!
//! This deliberately does not infer private report membership or source weights.
//! Captured presentation and current action state share one read snapshot.

use crate::{
    AppError,
    access::Level,
    auth::Session,
    store::{Attachment, Report},
};
use sqlx::PgPool;
use std::collections::HashMap;

const MAX_GROUPS: usize = 100;
const MAX_CATEGORIES: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportFilter {
    All,
    Board(String),
    Worksafe,
    NotWorksafe,
}

pub struct ReportQueuePage {
    pub groups: Vec<ReportGroup>,
    pub has_more: bool,
    pub filter: ReportFilter,
}

pub struct ReportGroup {
    pub representative: Report,
    pub has_open: bool,
    pub moderator_summary: Option<ModeratorReportSummary>,
    pub(crate) snapshot: Option<crate::report_post_snapshot::ReportPostSnapshot>,
}

pub struct ModeratorReportSummary {
    pub report_count: i64,
    pub categories: Vec<CapturedCategory>,
    pub categories_truncated: bool,
}

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
pub struct CapturedCategory {
    pub revision: i64,
    pub id: i64,
    pub kind: i16,
    pub title: String,
}

#[derive(sqlx::FromRow)]
struct SelectedGroup {
    board: String,
    post_id: i64,
    representative_id: i64,
    has_open: bool,
    report_count: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct CategoryRow {
    board: String,
    post_id: i64,
    #[sqlx(flatten)]
    category: CapturedCategory,
}

// Group BEFORE the resource cap. The 101st row is a lightweight sentinel only;
// its post body, categories and media are never fetched or rendered.
// Ordering is an explicit unweighted fallback, not the source's priority score.
const SELECT_GROUPS: &str = r#"
    WITH eligible AS (
        SELECT r.id,r.board,r.post_id,r.state
        FROM content.reports r
        JOIN content.posts p ON p.id=r.post_id AND p.board=r.board
        JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
        JOIN content.boards b ON b.slug=p.board
        WHERE ('all'=ANY($1) OR r.board=ANY($1)) AND NOT r.board=ANY($2)
          AND NOT p.deleted AND NOT t.deleted
          AND r.reporter_cleared_at IS NULL AND r.group_cleared_at IS NULL
          AND ($3::text IS NULL OR r.board=$3)
          AND ($4::boolean IS NULL OR b.worksafe=$4)
    )
    SELECT board,post_id,bool_or(state='open') AS has_open,max(id) AS latest_id,
           coalesce(max(id) FILTER (WHERE state='open'),max(id)) AS representative_id,
           CASE WHEN $5::boolean THEN count(*) ELSE NULL::bigint END AS report_count
    FROM eligible
    GROUP BY board,post_id
    ORDER BY has_open DESC,latest_id DESC,board,post_id
    LIMIT 101
"#;

// Keep role redaction at the SQL boundary: even the internal Janitor projection
// receives no representative reason or category fields.
const SELECT_PREVIEWS: &str = r#"
    SELECT r.id,r.board,r.post_id,p.thread_id,
           CASE WHEN $2::boolean THEN r.reason ELSE ''::text END AS reason,
           CASE WHEN $2::boolean THEN r.category_id ELSE NULL::bigint END AS category_id,
           CASE WHEN $2::boolean THEN r.category_kind ELSE NULL::smallint END AS category_kind,
           p.name,p.trip,p.poster_id,p.capcode,p.country,p.country_name,
           p.board_flag,p.board_flag_type,p.flag_name,p.subject,p.comment,
           p.drawing_time_seconds,p.drawing_source_post_id,p.comment_format,
           p.staff_authorized_limits,p.wordfilter_payload,p.saved_presentation,r.state,
           (t.closed OR t.archived_at IS NOT NULL) AS closed,
           t.sticky,t.permasage,t.permaage,t.undead,
           (t.archived_at IS NOT NULL) AS archived,
           (b.archive_retention_seconds>0) AS archives_enabled,
           (p.deleted OR t.deleted) AS deleted,
           b.comment_spoiler_cleanup AS spoilers_enabled,p.image_spoiler
    FROM content.reports r
    JOIN content.posts p ON p.id=r.post_id AND p.board=r.board
    JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
    JOIN content.boards b ON b.slug=p.board
    WHERE r.id=ANY($1)
"#;

// Captured reason is the admitted category title. Do not re-label old reports
// from a current catalog, or conflate equal IDs from different revisions/titles.
// Scan the selected retained inventory once. A correlated query per group
// would repeatedly scan content.reports, which has no uncleared board/post
// index. Keep DISTINCT and the per-group sentinel bound inside PostgreSQL.
const SELECT_CATEGORIES: &str = r#"
    WITH retained AS MATERIALIZED (
        SELECT r.board,r.post_id,r.category_revision AS revision,
               r.category_id AS id,r.category_kind AS kind,r.reason AS title
        FROM content.reports r
        WHERE r.reporter_cleared_at IS NULL AND r.group_cleared_at IS NULL
          AND r.category_id IS NOT NULL
          AND r.board=ANY($1::text[]) AND r.post_id=ANY($2::bigint[])
    ), categories AS (
        SELECT DISTINCT r.board,r.post_id,r.revision,r.id,r.kind,r.title
        FROM retained r
        JOIN unnest($1::text[],$2::bigint[]) AS g(board,post_id)
          ON r.board=g.board AND r.post_id=g.post_id
    ), ranked AS (
        SELECT *,row_number() OVER (
            PARTITION BY board,post_id ORDER BY revision,id,kind,title
        ) AS category_rank
        FROM categories
    )
    SELECT board,post_id,revision,id,kind,title
    FROM ranked WHERE category_rank<=17
    ORDER BY board,post_id,revision,id,kind,title
"#;

pub async fn load(
    pool: &PgPool,
    session: &Session,
    filter: ReportFilter,
) -> Result<ReportQueuePage, AppError> {
    load_inner(
        pool,
        session,
        filter,
        #[cfg(feature = "database-tests")]
        None,
    )
    .await
}

/// Per-call integration-test checkpoint after group selection establishes the
/// snapshot, before any preview, media or category projection. No global hooks
/// or database locks are installed, and this API is absent in production builds.
#[cfg(feature = "database-tests")]
pub async fn load_with_selection_checkpoint(
    pool: &PgPool,
    session: &Session,
    filter: ReportFilter,
    selected: &tokio::sync::Notify,
    resume: &tokio::sync::Notify,
) -> Result<ReportQueuePage, AppError> {
    load_inner(pool, session, filter, Some((selected, resume))).await
}

async fn load_inner(
    pool: &PgPool,
    session: &Session,
    filter: ReportFilter,
    #[cfg(feature = "database-tests")] checkpoint: Option<(
        &tokio::sync::Notify,
        &tokio::sync::Notify,
    )>,
) -> Result<ReportQueuePage, AppError> {
    if !session.at_least(Level::Janitor) {
        return Err(AppError::Forbidden);
    }
    let board = match &filter {
        ReportFilter::Board(board) => {
            // Deny before opening the transaction or checking board existence.
            if !session.permissions.allows(board) {
                return Err(AppError::Forbidden);
            }
            Some(board.as_str())
        }
        _ => None,
    };
    let worksafe = match &filter {
        ReportFilter::Worksafe => Some(true),
        ReportFilter::NotWorksafe => Some(false),
        _ => None,
    };
    let moderator = session.at_least(Level::Moderator);
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    crate::report_post_snapshot::ensure_ready(&mut tx).await?;
    if let Some(board) = board {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1)")
                .bind(board)
                .fetch_one(&mut *tx)
                .await?;
        if !exists {
            return Err(AppError::NotFound);
        }
    }
    let mut selected: Vec<SelectedGroup> = sqlx::query_as(SELECT_GROUPS)
        .bind(&session.permissions.allow_boards)
        .bind(&session.permissions.deny_boards)
        .bind(board)
        .bind(worksafe)
        .bind(moderator)
        .fetch_all(&mut *tx)
        .await?;
    let has_more = selected.len() > MAX_GROUPS;
    selected.truncate(MAX_GROUPS);
    #[cfg(feature = "database-tests")]
    if let Some((selected, resume)) = checkpoint {
        selected.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(10), resume.notified())
            .await
            .map_err(|_| AppError::Internal)?;
    }
    if selected.is_empty() {
        tx.commit().await?;
        return Ok(ReportQueuePage {
            groups: Vec::new(),
            has_more,
            filter,
        });
    }

    let representative_ids: Vec<i64> = selected.iter().map(|g| g.representative_id).collect();
    let mut snapshots = crate::report_post_snapshot::load(&mut tx, &representative_ids).await?;
    // Keep the transfer reader's aggregate materialization bound after grouped
    // selection. Count the complete saved bodies before fetching any payload.
    let bytes: i64 = sqlx::query_scalar(
        r#"
        SELECT coalesce(sum(octet_length(p.comment)::bigint
            +coalesce(octet_length(p.wordfilter_payload),0)
            +coalesce(octet_length(p.wordfilter_search),0)
            +coalesce(octet_length(p.saved_presentation),0)
            +coalesce(octet_length(p.saved_presentation_search),0)),0)::bigint
        FROM content.reports r JOIN content.posts p ON p.id=r.post_id AND p.board=r.board
        WHERE r.id=ANY($1)
    "#,
    )
    .bind(&representative_ids)
    .fetch_one(&mut *tx)
    .await?;
    const MAX_REPORT_READ_BYTES: usize = board_domain::MAX_COMMENT_BYTES * 1001;
    if bytes > MAX_REPORT_READ_BYTES as i64 {
        return Err(AppError::Capacity);
    }
    let reports: Vec<Report> = sqlx::query_as(SELECT_PREVIEWS)
        .bind(&representative_ids)
        .bind(moderator)
        .fetch_all(&mut *tx)
        .await?;
    let mut reports: HashMap<i64, Report> = reports.into_iter().map(|r| (r.id, r)).collect();
    let post_ids: Vec<i64> = selected.iter().map(|g| g.post_id).collect();
    let attachments: Vec<Attachment> = sqlx::query_as(
        "SELECT post_id,filename,bytes,width,height,spoiler,tim,thumbnail_width,thumbnail_height,available,md5,output_format FROM content.staff_post_media WHERE post_id=ANY($1)",
    )
    .bind(&post_ids)
    .fetch_all(&mut *tx)
    .await?;
    let mut attachments: HashMap<i64, Attachment> =
        attachments.into_iter().map(|a| (a.post_id, a)).collect();

    let mut categories: HashMap<(String, i64), Vec<CapturedCategory>> = HashMap::new();
    if moderator {
        // Never run this query for Janitors, including empty summaries.
        let boards: Vec<&str> = selected.iter().map(|g| g.board.as_str()).collect();
        let rows: Vec<CategoryRow> = sqlx::query_as(SELECT_CATEGORIES)
            .bind(&boards)
            .bind(&post_ids)
            .fetch_all(&mut *tx)
            .await?;
        for row in rows {
            categories
                .entry((row.board, row.post_id))
                .or_default()
                .push(row.category);
        }
    }

    let mut groups = Vec::with_capacity(selected.len());
    // SQL projection order is irrelevant: restore the selected group's order.
    for group in selected {
        let mut representative = reports
            .remove(&group.representative_id)
            .ok_or(AppError::Internal)?;
        if representative.board != group.board || representative.post_id != group.post_id {
            return Err(AppError::Internal);
        }
        representative.attachment = attachments.remove(&group.post_id);
        let snapshot = snapshots.remove(&group.representative_id);
        if snapshot
            .as_ref()
            .is_some_and(|s| !s.matches(&group.board, group.post_id))
        {
            return Err(AppError::Internal);
        }
        let moderator_summary = if moderator {
            let mut categories = categories
                .remove(&(group.board, group.post_id))
                .unwrap_or_default();
            let categories_truncated = categories.len() > MAX_CATEGORIES;
            categories.truncate(MAX_CATEGORIES);
            Some(ModeratorReportSummary {
                report_count: group.report_count.ok_or(AppError::Internal)?,
                categories,
                categories_truncated,
            })
        } else {
            None
        };
        groups.push(ReportGroup {
            representative,
            has_open: group.has_open,
            moderator_summary,
            snapshot,
        });
    }
    tx.commit().await?;
    Ok(ReportQueuePage {
        groups,
        has_more,
        filter,
    })
}
