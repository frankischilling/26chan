use crate::StoreError;
use sqlx::{PgConnection, PgPool};

/// Public identity of an eligible report target. No private report or session
/// data is read to render the report form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTarget {
    pub board: String,
    pub post_id: i64,
    pub thread_id: i64,
}

pub async fn report_target(pool: &PgPool, slug: &str, id: i64) -> Result<ReportTarget, StoreError> {
    let mut connection = pool.acquire().await?;
    report_target_on(&mut connection, slug, id).await
}

/// POST calls this again after taking the board mutation lock. GET and POST
/// share precedence and the existing RLS/retained-archive visibility rules.
pub(crate) async fn report_target_on(
    connection: &mut PgConnection,
    slug: &str,
    id: i64,
) -> Result<ReportTarget, StoreError> {
    let (enabled, post_id, thread_id, sticky, capcode): (
        bool,
        Option<i64>,
        Option<i64>,
        Option<bool>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT b.can_report_posts,p.id,t.id,t.sticky,p.capcode \
         FROM content.boards b \
         LEFT JOIN content.posts p ON p.board=b.slug AND p.id=$2 AND NOT p.deleted \
         LEFT JOIN content.visible_threads t ON t.board=p.board AND t.id=p.thread_id AND NOT t.deleted \
         WHERE b.slug=$1",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(StoreError::NotFound)?;
    if !enabled {
        return Err(StoreError::Invalid(
            "You cannot report posts on this board.",
        ));
    }
    let post_id = post_id.ok_or(StoreError::NotFound)?;
    let thread_id = thread_id.ok_or(StoreError::NotFound)?;
    // Source sticky belongs to the OP, not every reply in its thread.
    if post_id == thread_id && sticky == Some(true) {
        return Err(StoreError::Invalid("Error: You cannot report a sticky."));
    }
    if capcode.is_some() {
        return Err(StoreError::Invalid("Error: You cannot report this post."));
    }
    Ok(ReportTarget {
        board: slug.to_owned(),
        post_id,
        thread_id,
    })
}
