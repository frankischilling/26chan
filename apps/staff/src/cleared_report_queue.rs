//! Bounded, unweighted inventory of retained group-cleared reports.
//!
//! Report provenance is per record. This read does not inspect active membership,
//! infer old snapshots, or depend on the current post's availability.

use crate::{
    AppError, access::Level, auth::Session, report_queue::CapturedCategory, store::ClearedReport,
};
use sqlx::PgPool;
use std::collections::HashMap;

const MAX_GROUPS: usize = 100;
const MAX_ADDITIONAL_MEMBERS: usize = 15;
const MAX_CATEGORIES: usize = 16;

pub struct ClearedReportQueuePage {
    pub groups: Vec<ClearedReportGroup>,
    pub has_more: bool,
}

pub struct ClearedReportGroup {
    pub automatic_clear: bool,
    pub post_id: i64,
    pub representative: ClearedReport,
    pub moderator_summary: Option<ModeratorClearedReportSummary>,
    pub(crate) preview: Option<crate::views::Preview>,
}

pub struct ModeratorClearedReportSummary {
    pub report_count: i64,
    /// Other records after the representative, newest first. At most 15.
    pub additional_members: Vec<ClearedReport>,
    pub members_truncated: bool,
    pub categories: Vec<CapturedCategory>,
    pub categories_truncated: bool,
}

#[derive(sqlx::FromRow)]
struct SelectedGroup {
    automatic_clear: bool,
    post_id: i64,
    representative_id: i64,
}

#[derive(sqlx::FromRow)]
struct GroupCount {
    post_id: i64,
    report_count: i64,
}

#[derive(sqlx::FromRow)]
struct CategoryRow {
    post_id: i64,
    #[sqlx(flatten)]
    category: CapturedCategory,
}

// Apply clear membership before grouping and group before the preview bound.
// The newest eligible report represents its group. This deterministic order
// makes no claim about the source's unresolved weighted priority.
const SELECT_GROUPS: &str = r#"
    SELECT post_id,max(id) AS representative_id,CASE WHEN $2::boolean THEN bool_or(auto_cleared_at IS NOT NULL) ELSE false END AS automatic_clear
    FROM content.reports
    WHERE board=$1 AND (group_cleared_at IS NOT NULL OR auto_cleared_at IS NOT NULL) AND reporter_cleared_at IS NULL
    GROUP BY post_id
    ORDER BY representative_id DESC,post_id
    LIMIT 101
"#;

// Selected IDs are already the newest eligible representatives. Fetch only
// those at-most-100 records; never materialize member sentinels for Janitors.
const SELECT_REPRESENTATIVES: &str = r#"
    SELECT id,post_id,CASE WHEN $3::boolean THEN reason ELSE NULL::text END AS reason,
           CASE WHEN $3::boolean THEN category_id ELSE NULL::bigint END AS category_id,
           state,group_cleared_at,CASE WHEN $3::boolean THEN auto_cleared_at ELSE NULL::timestamptz END AS auto_cleared_at,
           CASE WHEN $3::boolean THEN group_cleared_by ELSE NULL::bigint END AS group_cleared_by,
           coalesce(group_clear_inherited,false) AS group_clear_inherited
    FROM content.reports
    WHERE board=$1 AND id=ANY($2::bigint[])
      AND (group_cleared_at IS NOT NULL OR auto_cleared_at IS NOT NULL) AND reporter_cleared_at IS NULL
"#;

const SELECT_COUNTS: &str = r#"
    SELECT post_id,count(*) AS report_count
    FROM content.reports
    WHERE board=$1 AND post_id=ANY($2)
      AND (group_cleared_at IS NOT NULL OR auto_cleared_at IS NOT NULL) AND reporter_cleared_at IS NULL
    GROUP BY post_id
"#;

// The representative plus 15 additional records is the display budget. The
// sixteenth additional record is a sentinel and is never returned to rendering.
const SELECT_ADDITIONAL_MEMBERS: &str = r#"
    WITH retained AS MATERIALIZED (
        SELECT id,post_id,reason,category_id,state,group_cleared_at,auto_cleared_at,
               group_cleared_by,coalesce(group_clear_inherited,false) AS group_clear_inherited
        FROM content.reports
        WHERE board=$1 AND post_id=ANY($2::bigint[])
          AND (group_cleared_at IS NOT NULL OR auto_cleared_at IS NOT NULL) AND reporter_cleared_at IS NULL
    ), ranked AS (
        SELECT r.*,row_number() OVER (
            PARTITION BY r.post_id ORDER BY r.id DESC
        ) AS member_rank
        FROM retained r
        JOIN unnest($2::bigint[],$3::bigint[]) AS g(post_id,representative_id)
          ON r.post_id=g.post_id AND r.id<g.representative_id
    )
    SELECT id,post_id,reason,category_id,state,group_cleared_at,auto_cleared_at,
           group_cleared_by,coalesce(group_clear_inherited,false) AS group_clear_inherited
    FROM ranked WHERE member_rank<=16
    ORDER BY post_id,id DESC
"#;

// Categories have their own bound across ALL eligible retained members, rather
// than only the small member preview. Captured revisions/titles stay distinct.
// The cleared-history index starts with board/id, not post_id. Materialize
// eligible selected rows once instead of rescanning that board for each group.
const SELECT_CATEGORIES: &str = r#"
    WITH retained AS MATERIALIZED (
        SELECT post_id,category_revision AS revision,category_id AS id,
               category_kind AS kind,reason AS title
        FROM content.reports
        WHERE board=$1 AND post_id=ANY($2::bigint[])
          AND (group_cleared_at IS NOT NULL OR auto_cleared_at IS NOT NULL) AND reporter_cleared_at IS NULL
          AND category_id IS NOT NULL
    ), categories AS (
        SELECT DISTINCT post_id,revision,id,kind,title FROM retained
    ), ranked AS (
        SELECT *,row_number() OVER (
            PARTITION BY post_id ORDER BY revision,id,kind,title
        ) AS category_rank
        FROM categories
    )
    SELECT post_id,revision,id,kind,title
    FROM ranked WHERE category_rank<=17
    ORDER BY post_id,revision,id,kind,title
"#;

pub async fn load(
    pool: &PgPool,
    session: &Session,
    board: &str,
) -> Result<ClearedReportQueuePage, AppError> {
    load_inner(
        pool,
        session,
        board,
        #[cfg(feature = "database-tests")]
        None,
    )
    .await
}

/// Per-call test checkpoint after selection establishes the read snapshot.
/// Production builds contain neither the checkpoint nor its waiting path.
#[cfg(feature = "database-tests")]
pub async fn load_with_selection_checkpoint(
    pool: &PgPool,
    session: &Session,
    board: &str,
    selected: &tokio::sync::Notify,
    resume: &tokio::sync::Notify,
) -> Result<ClearedReportQueuePage, AppError> {
    load_inner(pool, session, board, Some((selected, resume))).await
}

async fn load_inner(
    pool: &PgPool,
    session: &Session,
    board: &str,
    #[cfg(feature = "database-tests")] checkpoint: Option<(
        &tokio::sync::Notify,
        &tokio::sync::Notify,
    )>,
) -> Result<ClearedReportQueuePage, AppError> {
    if !session.at_least(Level::Janitor) || !session.permissions.allows(board) {
        return Err(AppError::Forbidden);
    }
    board_domain::BoardSlug::parse(board).map_err(|_| AppError::Invalid)?;
    let moderator = session.at_least(Level::Moderator);
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    crate::report_post_snapshot::ensure_ready(&mut tx).await?;
    let mut selected: Vec<SelectedGroup> = sqlx::query_as(SELECT_GROUPS)
        .bind(board)
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
        return Ok(ClearedReportQueuePage {
            groups: Vec::new(),
            has_more,
        });
    }

    let post_ids: Vec<i64> = selected.iter().map(|g| g.post_id).collect();
    let representative_ids: Vec<i64> = selected.iter().map(|g| g.representative_id).collect();
    let mut snapshots = crate::report_post_snapshot::load(&mut tx, &representative_ids).await?;
    let representatives: Vec<ClearedReport> = sqlx::query_as(SELECT_REPRESENTATIVES)
        .bind(board)
        .bind(&representative_ids)
        .bind(moderator)
        .fetch_all(&mut *tx)
        .await?;
    let mut representatives: HashMap<i64, ClearedReport> = representatives
        .into_iter()
        .map(|r| (r.post_id, r))
        .collect();
    let mut counts = HashMap::new();
    let mut members: HashMap<i64, Vec<ClearedReport>> = HashMap::new();
    let mut categories: HashMap<i64, Vec<CapturedCategory>> = HashMap::new();
    if moderator {
        // These projections and their sentinels are never executed for Janitors.
        let rows: Vec<GroupCount> = sqlx::query_as(SELECT_COUNTS)
            .bind(board)
            .bind(&post_ids)
            .fetch_all(&mut *tx)
            .await?;
        counts.extend(rows.into_iter().map(|r| (r.post_id, r.report_count)));
        let rows: Vec<ClearedReport> = sqlx::query_as(SELECT_ADDITIONAL_MEMBERS)
            .bind(board)
            .bind(&post_ids)
            .bind(&representative_ids)
            .fetch_all(&mut *tx)
            .await?;
        for row in rows {
            members.entry(row.post_id).or_default().push(row);
        }
        let rows: Vec<CategoryRow> = sqlx::query_as(SELECT_CATEGORIES)
            .bind(board)
            .bind(&post_ids)
            .fetch_all(&mut *tx)
            .await?;
        for row in rows {
            categories
                .entry(row.post_id)
                .or_default()
                .push(row.category);
        }
    }

    let mut groups = Vec::with_capacity(selected.len());
    for group in selected {
        let representative = representatives
            .remove(&group.post_id)
            .ok_or(AppError::Internal)?;
        if representative.id != group.representative_id {
            return Err(AppError::Internal);
        }
        let preview = snapshots
            .remove(&group.representative_id)
            .map(|s| {
                if !s.matches(board, group.post_id) {
                    return Err(AppError::Internal);
                }
                Ok(s.history_preview(group.representative_id, representative.state.clone()))
            })
            .transpose()?;
        let moderator_summary = if moderator {
            let mut additional_members = members.remove(&group.post_id).unwrap_or_default();
            let members_truncated = additional_members.len() > MAX_ADDITIONAL_MEMBERS;
            additional_members.truncate(MAX_ADDITIONAL_MEMBERS);
            let mut categories = categories.remove(&group.post_id).unwrap_or_default();
            let categories_truncated = categories.len() > MAX_CATEGORIES;
            categories.truncate(MAX_CATEGORIES);
            Some(ModeratorClearedReportSummary {
                report_count: counts.remove(&group.post_id).ok_or(AppError::Internal)?,
                additional_members,
                members_truncated,
                categories,
                categories_truncated,
            })
        } else {
            None
        };
        groups.push(ClearedReportGroup {
            automatic_clear: group.automatic_clear,
            post_id: group.post_id,
            representative,
            moderator_summary,
            preview,
        });
    }
    tx.commit().await?;
    Ok(ClearedReportQueuePage { groups, has_more })
}
