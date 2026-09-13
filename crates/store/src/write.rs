use crate::*;

#[derive(Clone)]
pub struct NewPost {
    pub name: String,
    pub subject: String,
    pub comment: String,
    pub deletion_hash: String,
    pub sage: bool,
}

pub async fn create_post(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
) -> Result<i64, StoreError> {
    create_post_with_attachment(pool, slug, parent, post, None).await
}

pub async fn create_post_with_attachment(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
) -> Result<i64, StoreError> {
    let mut tx = pool.begin().await?;
    let board: Board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    board_domain::validate_post(
        &post.name,
        &post.subject,
        &post.comment,
        board.max_comment_chars as usize,
    )
    .map_err(|error| StoreError::Invalid(error.0))?;
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await?;
    let thread_id = if parent == 0 {
        crate::archives::make_room(&mut tx, &board).await?;
        sqlx::query("INSERT INTO content.threads(id,board) VALUES ($1,$2)")
            .bind(id)
            .bind(slug)
            .execute(&mut *tx)
            .await?;
        id
    } else {
        let thread: Thread = sqlx::query_as(
            "SELECT * FROM content.threads WHERE board=$1 AND id=$2 AND NOT deleted AND EXISTS (SELECT 1 FROM content.visible_threads WHERE board=$1 AND id=$2) FOR UPDATE",
        )
        .bind(slug)
        .bind(parent)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        if thread.archived_at.is_some() || thread.closed || thread.reply_count >= board.reply_limit
        {
            return Err(StoreError::Conflict(
                "This thread is closed or has reached its reply limit.",
            ));
        }
        let bump = !post.sage && thread.reply_count < board.bump_limit;
        sqlx::query("UPDATE content.threads SET reply_count=reply_count+1, modified_at=clock_timestamp(), bumped_at=CASE WHEN $2 THEN clock_timestamp() ELSE bumped_at END WHERE id=$1").bind(parent).bind(bump).execute(&mut *tx).await?;
        parent
    };
    let name = if post.name.trim().is_empty() {
        "Anonymous"
    } else {
        post.name.trim()
    };
    if let Some(attachment) = attachment {
        sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(id)
            .bind(slug)
            .bind(thread_id)
            .bind(name)
            .bind(&post.subject)
            .bind(&post.comment)
            .bind(&attachment.upload.id)
            .bind(&attachment.upload.capability)
            .bind(attachment.spoiler)
            .execute(&mut *tx)
            .await
            .map_err(post_media::scoped_error)?;
    } else {
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$3,$4,$5,$6)").bind(id).bind(slug).bind(thread_id).bind(name).bind(&post.subject).bind(&post.comment).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES ($1,$2)")
        .bind(id)
        .bind(&post.deletion_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// Caller must verify the deletion password before entering this operation.
/// The board/post relationship is checked again under the mutation lock.
pub async fn delete_post(pool: &PgPool, slug: &str, id: i64) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let post: Post = sqlx::query_as(
        "SELECT * FROM content.posts p WHERE board=$1 AND id=$2 AND NOT deleted AND EXISTS (SELECT 1 FROM content.visible_threads t WHERE t.board=p.board AND t.id=p.thread_id) FOR UPDATE",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    if id == post.thread_id {
        sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(slug).bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND thread_id=$2")
            .bind(slug)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(slug)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=$1 AND id=$2",
        )
        .bind(slug)
        .bind(post.thread_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn report(pool: &PgPool, slug: &str, id: i64, reason: &str) -> Result<(), StoreError> {
    if reason.trim().is_empty() || reason.len() > 1000 || reason.contains('\0') {
        return Err(StoreError::Invalid(
            "Report reason must contain 1 to 1000 bytes.",
        ));
    }
    let mut tx = pool.begin().await?;
    // Serialize against deletion and ensure reporting cannot target another board.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    sqlx::query("SELECT p.id FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted AND NOT t.deleted").bind(slug).bind(id).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
    sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,$3)")
        .bind(slug)
        .bind(id)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
