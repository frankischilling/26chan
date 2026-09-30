use crate::{PgPool, StoreError};

#[derive(sqlx::FromRow)]
pub struct ThreadStatistics {
    pub board: String,
    pub id: i64,
    pub replies: i64,
    pub images: i64,
    pub unique_ips: Option<i32>,
    pub sticky: bool,
    pub closed: bool,
    pub archived: bool,
    pub permaage: bool,
    pub undead: bool,
    pub bump_limit: i32,
    pub image_limit: i32,
    pub page: Option<i64>,
}

/// One SELECT gives counts, board policy and ordering the same MVCC snapshot.
/// No comment body or private poster identity crosses the database boundary.
pub async fn thread_statistics(
    pool: &PgPool,
    board: &str,
    id: i64,
) -> Result<ThreadStatistics, StoreError> {
    board_domain::BoardSlug::parse(board).map_err(|_| StoreError::NotFound)?;
    if id <= 0 {
        return Err(StoreError::NotFound);
    }
    let result: ThreadStatistics = sqlx::query_as(
        "SELECT t.board,t.id,t.sticky,t.closed,t.permaage,t.undead,
                (t.archived_at IS NOT NULL) AS archived,b.bump_limit,b.image_limit,
                counts.replies,counts.images,content.unique_posters(t.board,t.id) AS unique_ips,
                CASE WHEN t.archived_at IS NOT NULL THEN NULL ELSE
                  1 + (SELECT count(*) FROM content.visible_threads earlier
                    WHERE earlier.board=t.board AND NOT earlier.deleted AND earlier.archived_at IS NULL
                      AND (earlier.sticky,earlier.bumped_at,earlier.id) > (t.sticky,t.bumped_at,t.id)) / b.threads_per_page
                END AS page
         FROM content.visible_threads t JOIN content.boards b ON b.slug=t.board
         CROSS JOIN LATERAL (
           SELECT count(*) AS replies,
             count(*) FILTER (WHERE m.post_id IS NOT NULL AND NOT m.file_deleted) AS images
           FROM content.posts p LEFT JOIN content.visible_post_media m ON m.post_id=p.id
           WHERE p.board=t.board AND p.thread_id=t.id AND p.id<>t.id AND NOT p.deleted
         ) counts
         WHERE t.board=$1 AND t.id=$2 AND NOT t.deleted",
    ).bind(board).bind(id).fetch_optional(pool).await?.ok_or(StoreError::NotFound)?;
    if !(0..=1000).contains(&result.replies)
        || !(0..=result.replies).contains(&result.images)
        || result.unique_ips.is_some_and(|count| {
            i64::from(count) < 1 || i64::from(count) > result.replies + 1 || result.archived
        })
        || result.bump_limit < 0
        || result.image_limit < 0
        || (!result.archived && !result.page.is_some_and(|page| (1..=1000).contains(&page)))
        || (result.archived && result.page.is_some())
    {
        return Err(StoreError::Invalid("Thread statistics exceed limits."));
    }
    Ok(result)
}
