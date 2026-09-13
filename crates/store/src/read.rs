use crate::*;

/// Count rows without transferring comment bodies to a web process.
pub async fn visible_post_count(pool: &PgPool, slug: &str, id: i64) -> Result<i64, StoreError> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.thread_id=$2 AND NOT p.deleted AND NOT t.deleted").bind(slug).bind(id).fetch_one(pool).await?)
}

/// Fetch only the OP and at most five latest replies. Limits apply in SQL.
pub async fn preview_posts(
    pool: &PgPool,
    slug: &str,
    id: i64,
    replies: i64,
) -> Result<Vec<Post>, StoreError> {
    if !(0..=5).contains(&replies) {
        return Err(StoreError::Invalid("Invalid preview limit."));
    }
    Ok(sqlx::query_as("SELECT p.* FROM ((SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND id=$2 AND NOT deleted) UNION ALL (SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id DESC LIMIT $3)) p WHERE EXISTS (SELECT 1 FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted) ORDER BY p.id").bind(slug).bind(id).bind(replies).fetch_all(pool).await?)
}

pub async fn boards(pool: &PgPool) -> Result<Vec<Board>, StoreError> {
    Ok(
        sqlx::query_as("SELECT * FROM content.boards ORDER BY slug LIMIT 100")
            .fetch_all(pool)
            .await?,
    )
}
pub async fn board(pool: &PgPool, slug: &str) -> Result<Board, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}
pub async fn thread(pool: &PgPool, slug: &str, id: i64) -> Result<Thread, StoreError> {
    sqlx::query_as("SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted")
        .bind(slug)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}
pub struct ThreadSnapshot {
    pub board: Board,
    pub thread: Thread,
    pub posts: Vec<Post>,
}

/// Read board settings, thread metadata and posts from one database snapshot.
/// Release the transaction before the caller renders the representation.
pub async fn thread_snapshot(
    pool: &PgPool,
    slug: &str,
    id: i64,
) -> Result<ThreadSnapshot, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let metadata = sqlx::query_as(
        "SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    let mut entries = sqlx::query_as("SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND NOT deleted ORDER BY id LIMIT 1001")
        .bind(slug)
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    crate::post_media::load(&mut tx, &mut entries).await?;
    tx.commit().await?;
    Ok(ThreadSnapshot {
        board,
        thread: metadata,
        posts: entries,
    })
}
pub async fn threads(
    pool: &PgPool,
    slug: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<Thread>, StoreError> {
    if !(0..=1000).contains(&offset) || !(1..=1000).contains(&limit) {
        return Err(StoreError::NotFound);
    }
    Ok(sqlx::query_as("SELECT * FROM content.visible_threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL ORDER BY sticky DESC,bumped_at DESC,id DESC OFFSET $2 LIMIT $3").bind(slug).bind(offset).bind(limit).fetch_all(pool).await?)
}
pub async fn posts(pool: &PgPool, slug: &str, id: i64) -> Result<Vec<Post>, StoreError> {
    Ok(sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.thread_id=$2 AND NOT p.deleted AND NOT t.deleted ORDER BY p.id LIMIT 1001").bind(slug).bind(id).fetch_all(pool).await?)
}
pub async fn find_post(pool: &PgPool, slug: &str, id: i64) -> Result<Post, StoreError> {
    sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted AND NOT t.deleted").bind(slug).bind(id).fetch_optional(pool).await?.ok_or(StoreError::NotFound)
}
pub async fn deletion_hash(pool: &PgPool, slug: &str, id: i64) -> Result<String, StoreError> {
    let post = find_post(pool, slug, id).await?;
    sqlx::query_scalar("SELECT password_hash FROM post_secrets.deletion WHERE post_id=$1")
        .bind(post.id)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}
