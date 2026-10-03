use crate::{Board, PgPool, Post, StoreError};

pub struct RssSnapshot {
    pub board: Board,
    pub posts: Vec<Post>,
}

/// The feed selects the newest twenty live OPs by number, independently of
/// sticky and bump order. Settings, comments and media share one snapshot.
pub async fn rss_snapshot(pool: &PgPool, slug: &str) -> Result<RssSnapshot, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board: Board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1 AND rss_enabled")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let mut posts: Vec<Post> = sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=p.thread_id AND NOT p.deleted AND t.archived_at IS NULL ORDER BY p.id DESC LIMIT 20")
        .bind(slug)
        .fetch_all(&mut *tx)
        .await?;
    crate::post_media::load(&mut tx, &mut posts).await?;
    tx.commit().await?;
    Ok(RssSnapshot { board, posts })
}
