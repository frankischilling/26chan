use crate::*;
use chrono::Timelike;

#[derive(Clone)]
pub struct NewPost {
    pub name: String,
    pub subject: String,
    pub comment: String,
    pub deletion_hash: String,
    pub sage: bool,
}

/// Server-owned posting context; never deserialize this from a request body.
#[derive(Clone, Copy)]
pub struct PostingContext {
    pub request_start: DateTime<Utc>,
    pub peer: Option<std::net::IpAddr>,
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
    create_post_with_attachment_at(pool, slug, parent, post, attachment, Utc::now()).await
}

/// Internal callers supply a server-owned request start, never a client field.
/// Capture it before parsing, hashing, pool acquisition or mutation lock waits.
pub async fn create_post_with_attachment_at(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    request_start: DateTime<Utc>,
) -> Result<i64, StoreError> {
    create_post_with_context(
        pool,
        slug,
        parent,
        post,
        attachment,
        PostingContext {
            request_start,
            peer: None,
        },
    )
    .await
}

pub async fn create_post_with_context(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: PostingContext,
) -> Result<i64, StoreError> {
    let comment = board_domain::normalize_comment(&post.comment)
        .map_err(|error| StoreError::Invalid(error.0))?;
    let posted_at = context
        .request_start
        .with_nanosecond(0)
        .ok_or(StoreError::Invalid("Invalid posting timestamp."))?;
    let mut tx = pool.begin().await?;
    let board: Board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let board_domain::PreparedPostContent { comment, subject } =
        board_domain::prepare_post_content(
            &post.name,
            &post.subject,
            &comment,
            board.max_comment_chars as usize,
            attachment.is_some(),
            board.comment_spacing(),
            board.require_subject && parent == 0,
        )
        .map_err(|error| StoreError::Invalid(error.0))?;
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await?;
    let mut own_reply = false;
    let peer = context.peer.map(|peer| peer.to_canonical().to_string());
    let thread_id = if parent == 0 {
        crate::archives::make_room(&mut tx, &board).await?;
        sqlx::query(
            "INSERT INTO content.threads(id,board,created_at,modified_at) VALUES ($1,$2,$3,$3)",
        )
        .bind(id)
        .bind(slug)
        .bind(posted_at)
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
        // Count under the same board lock as posting/deletion. The incoming
        // row is not inserted yet; the source's decision includes that reply.
        let (replies, op_created): (i64, DateTime<Utc>) = sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted),created_at FROM content.posts WHERE board=$1 AND id=$2 AND NOT deleted")
            .bind(slug).bind(parent).fetch_one(&mut *tx).await?;
        if let Some(peer) = &peer {
            own_reply = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=$1 AND peer=$2::text::inet)")
                .bind(parent).bind(peer).fetch_one(&mut *tx).await?;
        }
        let mut self_sage = false;
        if own_reply && board.op_bump_limit {
            let latest: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT p.created_at FROM post_secrets.op_replies r JOIN content.posts p ON p.id=r.post_id WHERE r.thread_id=$1 AND p.board=$2 AND p.thread_id=$1 AND NOT p.deleted ORDER BY p.id DESC LIMIT 1")
                .bind(parent).bind(slug).fetch_optional(&mut *tx).await?;
            self_sage = board_domain::op_bump::limited(
                true,
                context.request_start.timestamp(),
                op_created.timestamp(),
                latest.map(|time| time.timestamp()),
                board.op_bump_initial_seconds as u32,
                board.op_bump_repeat_seconds as u32,
            );
        }
        let bump = board_domain::bump::should_bump(
            thread.sticky,
            thread.permasage,
            thread.permaage,
            post.sage || self_sage,
            replies as u64 + 1,
            board.bump_limit as u32,
            board_domain::bump::age_limited(
                context.request_start.timestamp(),
                op_created.timestamp(),
                board.permasage_hours as u32,
            ),
        );
        sqlx::query("UPDATE content.threads SET reply_count=reply_count+1, modified_at=$3, bumped_at=CASE WHEN $2 THEN clock_timestamp() ELSE bumped_at END WHERE id=$1").bind(parent).bind(bump).bind(posted_at).execute(&mut *tx).await?;
        parent
    };
    let name = if post.name.trim().is_empty() {
        "Anonymous"
    } else {
        post.name.trim()
    };
    if let Some(attachment) = attachment {
        sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(id)
            .bind(slug)
            .bind(thread_id)
            .bind(name)
            .bind(&subject)
            .bind(comment.as_str())
            .bind(&attachment.upload.id)
            .bind(&attachment.upload.capability)
            .bind(attachment.spoiler)
            .bind(posted_at)
            .execute(&mut *tx)
            .await
            .map_err(post_media::scoped_error)?;
    } else {
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(slug).bind(thread_id).bind(name).bind(&subject).bind(comment.as_str()).bind(posted_at).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES ($1,$2)")
        .bind(id)
        .bind(&post.deletion_hash)
        .execute(&mut *tx)
        .await?;
    if parent == 0 {
        if let Some(peer) = peer {
            sqlx::query(
                "INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES($1,$2::text::inet)",
            )
            .bind(id)
            .bind(peer)
            .execute(&mut *tx)
            .await?;
        }
    } else if own_reply {
        sqlx::query("INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES($1,$2)")
            .bind(id)
            .bind(parent)
            .execute(&mut *tx)
            .await?;
    }
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
