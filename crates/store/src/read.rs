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
    pub replies: usize,
    pub images: usize,
    pub tail_size: usize,
    pub tail_id: Option<i64>,
}

/// Read board settings, thread metadata and posts from one database snapshot.
/// Release the transaction before the caller renders the representation.
pub async fn thread_snapshot(
    pool: &PgPool,
    slug: &str,
    id: i64,
) -> Result<ThreadSnapshot, StoreError> {
    thread_snapshot_selection(pool, slug, id, false).await
}

/// Full counts, policy, boundary and selected posts share one read transaction.
pub async fn thread_snapshot_selection(
    pool: &PgPool,
    slug: &str,
    id: i64,
    tail: bool,
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
    let metadata: Thread = sqlx::query_as(
        "SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    let (configured, undead): (i32, bool) = sqlx::query_as("SELECT b.json_tail_size,t.undead FROM content.boards b JOIN content.threads t ON t.board=b.slug WHERE b.slug=$1 AND t.id=$2")
        .bind(slug).bind(id).fetch_one(&mut *tx).await?;
    let (reply_count, image_count): (i64, i64) = sqlx::query_as("SELECT count(*),count(*) FILTER (WHERE m.post_id IS NOT NULL AND NOT m.file_deleted) FROM content.posts p LEFT JOIN content.visible_post_media m ON m.post_id=p.id WHERE p.board=$1 AND p.thread_id=$2 AND p.id<>$2 AND NOT p.deleted")
        .bind(slug).bind(id).fetch_one(&mut *tx).await?;
    if !(0..=1000).contains(&reply_count) || !(0..=500).contains(&configured) {
        return Err(StoreError::Invalid("Thread exceeds snapshot limits."));
    }
    let replies = reply_count as usize;
    let images = image_count as usize;
    let tail_size =
        board_domain::thread_tail_size(configured as u16, metadata.sticky, undead, replies);
    if tail && tail_size == 0 {
        return Err(StoreError::NotFound);
    }
    let tail_id = if tail {
        Some(sqlx::query_scalar::<_, i64>("SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id DESC OFFSET $3 LIMIT 1")
            .bind(slug).bind(id).bind(tail_size as i64).fetch_one(&mut *tx).await?)
    } else {
        None
    };
    let mut entries = sqlx::query_as("SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND NOT deleted AND ($3::bigint IS NULL OR id=$2 OR id>$3) ORDER BY id LIMIT 1001")
        .bind(slug)
        .bind(id)
        .bind(tail_id)
        .fetch_all(&mut *tx)
        .await?;
    let expected = 1 + if tail { tail_size } else { replies };
    if entries.len() != expected {
        return Err(StoreError::Invalid("Incomplete thread snapshot."));
    }
    crate::post_media::load(&mut tx, &mut entries).await?;
    tx.commit().await?;
    Ok(ThreadSnapshot {
        board,
        thread: metadata,
        posts: entries,
        replies,
        images,
        tail_size,
        tail_id,
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

/// Missing historical password state cannot establish OP ownership.
pub(crate) const OP_DELETION_HASH: &str = "SELECT d.password_hash FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=$2 AND p.id=p.thread_id AND NOT p.deleted";

pub async fn op_deletion_hash(
    pool: &PgPool,
    slug: &str,
    parent: i64,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(OP_DELETION_HASH)
        .bind(slug)
        .bind(parent)
        .fetch_optional(pool)
        .await?)
}
