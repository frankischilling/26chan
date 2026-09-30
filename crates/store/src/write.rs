use crate::*;
use chrono::Timelike;
use sha2::{Digest, Sha256};

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
    /// Fingerprint of the OP hash actually verified by the server; never client input.
    pub op_password_proof: Option<[u8; 32]>,
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
            op_password_proof: None,
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
    create_post_with_context_and_key(pool, slug, parent, post, attachment, context, None).await
}

pub async fn create_post_with_context_and_key(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: PostingContext,
    tripcode_key: Option<&board_domain::identity::SecureKey>,
) -> Result<i64, StoreError> {
    create_post_with_identity_keys(
        pool,
        slug,
        parent,
        post,
        attachment,
        context,
        PostIdentityKeys {
            tripcode: tripcode_key,
            poster_id: None,
        },
    )
    .await
}

/// Deployment keys stay outside request fields and transport identity.
pub struct PostIdentityKeys<'a> {
    pub tripcode: Option<&'a board_domain::identity::SecureKey>,
    pub poster_id: Option<&'a board_domain::poster_id::PosterIdKey>,
}

pub async fn create_post_with_identity_keys(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: PostingContext,
    keys: PostIdentityKeys<'_>,
) -> Result<i64, StoreError> {
    create_post_with_metadata(
        pool,
        slug,
        parent,
        post,
        attachment,
        context,
        PostMetadata {
            keys,
            country_database: None,
            flag: "",
        },
    )
    .await
}

pub struct PostMetadata<'a> {
    pub keys: PostIdentityKeys<'a>,
    pub country_database: Option<&'a board_domain::country::CountryDatabase>,
    /// Public choice, validated against the locked operator-owned board policy.
    pub flag: &'a str,
}

pub async fn create_post_with_metadata(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: PostingContext,
    metadata: PostMetadata<'_>,
) -> Result<i64, StoreError> {
    create_post_in_context(
        pool,
        slug,
        parent,
        post,
        attachment,
        context,
        PostWriteOptions {
            metadata,
            staff: None,
        },
    )
    .await
}

/// Server-owned WebAuthn session proof. Request bodies cannot supply this value.
pub struct StaffPostAuthority<'a> {
    pub auth_pool: &'a PgPool,
    pub session_hash: &'a [u8],
    pub csrf_hash: &'a [u8],
    pub ticket_hash: &'a [u8; 32],
    pub idle_seconds: i32,
    pub highlight: bool,
}

pub async fn create_staff_post(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    request_start: DateTime<Utc>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    create_post_in_context(
        pool,
        slug,
        parent,
        post,
        None,
        PostingContext {
            request_start,
            peer: None,
            op_password_proof: None,
        },
        PostWriteOptions {
            metadata: PostMetadata {
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: None,
                },
                country_database: None,
                flag: "",
            },
            staff: Some(authority),
        },
    )
    .await
}

struct PostWriteOptions<'a> {
    metadata: PostMetadata<'a>,
    staff: Option<StaffPostAuthority<'a>>,
}

async fn create_post_in_context(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: PostingContext,
    options: PostWriteOptions<'_>,
) -> Result<i64, StoreError> {
    let PostWriteOptions { metadata, staff } = options;
    let keys = metadata.keys;
    let comment = board_domain::normalize_comment(&post.comment)
        .map_err(|error| StoreError::Invalid(error.0))?;
    let posted_at = context
        .request_start
        .with_nanosecond(0)
        .ok_or(StoreError::Invalid("Invalid posting timestamp."))?;
    let mut tx = pool.begin().await?;
    if staff.is_some() {
        let role: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&mut *tx)
            .await?;
        if role != "board_staff" {
            return Err(StoreError::UnsafeRole);
        }
    }
    let board: Board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    board.check_attachment_allowed(parent, attachment.is_some())?;
    let flag = if metadata.flag == "0" {
        ""
    } else {
        metadata.flag
    };
    if !flag.is_empty()
        && (!board.board_flags.iter().any(|enabled| enabled == flag)
            || board_domain::country::board_flag(flag).is_none())
    {
        return Err(StoreError::Invalid("Invalid board flag."));
    }
    let country = if staff.is_none() && board.country_flags && flag.is_empty() {
        let database = metadata
            .country_database
            .ok_or(StoreError::Invalid("Country flags are unavailable."))?;
        let peer = context.peer.ok_or(StoreError::Invalid(
            "Posting transport identity is unavailable.",
        ))?;
        Some(
            database
                .lookup(peer)
                .map_err(|error| StoreError::Invalid(error.0))?,
        )
    } else {
        None
    };
    sqlx::query("SELECT set_config('board.country', $1, true),set_config('board.country_name', $2, true),set_config('board.flag', $3, true)")
        .bind(country.as_ref().map_or("", |value| value.code.as_str()))
        .bind(country.as_ref().map_or("", |value| value.name.as_str()))
        .bind(flag).execute(&mut *tx).await?;
    let peer = context.peer.map(|peer| peer.to_canonical().to_string());
    let own_reply = if parent > 0 {
        if let Some(peer) = &peer {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=$1 AND peer=$2::text::inet)")
                .bind(parent).bind(peer).fetch_one(&mut *tx).await?
        } else {
            false
        }
    } else {
        false
    };
    let password_matches = if staff.is_none() && board.op_markup && parent > 0 {
        if let Some(proof) = context.op_password_proof {
            let hash: Option<String> = sqlx::query_scalar(crate::read::OP_DELETION_HASH)
                .bind(slug)
                .bind(parent)
                .fetch_optional(&mut *tx)
                .await?;
            hash.is_some_and(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())) == proof)
        } else {
            false
        }
    } else {
        false
    };
    let op_markup = board.op_markup && (parent == 0 || own_reply || password_matches);
    // Source clears identity before required-subject and final content checks.
    // Retain the raw input bounds even when these fields will be discarded.
    let (post_name, post_subject) = if board.forced_anon {
        board_domain::validate_post_with_attachment(
            &post.name,
            &post.subject,
            &comment,
            board.max_comment_chars as usize,
            true,
        )
        .map_err(|error| StoreError::Invalid(error.0))?;
        ("Anonymous", "")
    } else {
        (post.name.as_str(), post.subject.as_str())
    };
    let board_domain::PreparedPostContent { comment, subject } =
        board_domain::prepare_post_content(
            post_name,
            post_subject,
            &comment,
            board.max_comment_chars as usize,
            attachment.is_some(),
            board.comment_spacing().with_op_markup(op_markup),
            if parent == 0 {
                board_domain::PostKind::Thread {
                    subject_required: board.require_subject,
                    text_only: board.text_only,
                }
            } else {
                board_domain::PostKind::Reply
            },
        )
        .map_err(|error| StoreError::Invalid(error.0))?;
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await?;
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
    let poster_id = if staff.is_none() && board.user_ids {
        let key = keys
            .poster_id
            .ok_or(StoreError::Invalid("Poster IDs are unavailable."))?;
        let peer = context
            .peer
            .ok_or(StoreError::Invalid("Poster identity is unavailable."))?;
        Some(
            key.label(slug, thread_id, peer)
                .map_err(|error| StoreError::Invalid(error.0))?,
        )
    } else {
        None
    };
    sqlx::query("SELECT set_config('board.poster_id', $1, true)")
        .bind(poster_id.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;
    let count_context = keys
        .poster_id
        .zip(context.peer)
        .map(|(key, peer)| key.count_context(slug, thread_id, peer))
        .transpose()
        .map_err(|error| StoreError::Invalid(error.0))?;
    sqlx::query("SELECT set_config('board.poster_fingerprint', $1, true),set_config('board.poster_epoch', $2, true)")
        .bind(count_context.as_ref().map_or("", |value| value.fingerprint.as_str()))
        .bind(count_context.as_ref().map_or("", |value| value.epoch.as_str()))
        .execute(&mut *tx).await?;
    let identity = if staff.is_some() {
        // Staff badges replace trips. Never publish a suffix entered using the
        // public form's private trip-password syntax.
        let display = post_name.split('#').next().unwrap_or("").trim();
        board_domain::identity::Identity {
            name: if display.is_empty() {
                "Anonymous"
            } else {
                display
            }
            .into(),
            trip: None,
        }
    } else {
        board_domain::identity::prepare(post_name, keys.tripcode)
            .map_err(|error| StoreError::Invalid(error.0))?
    };
    sqlx::query("SELECT set_config('board.post_trip', $1, true)")
        .bind(identity.trip.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;
    let name = identity.name.as_str();
    // Cosmetic source eligibility is supplied by this server-owned context.
    // SET LOCAL cannot leak to a later request on the pooled connection. The
    // trigger independently locks and checks the operator's board setting.
    sqlx::query("SELECT set_config('board.source_op_reply', $1, true)")
        .bind(if op_markup { "true" } else { "false" })
        .execute(&mut *tx)
        .await?;
    if let Some(authority) = &staff {
        sqlx::query(
            "SELECT staff_identity.issue_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(authority.ticket_hash.as_slice())
        .bind(authority.session_hash)
        .bind(authority.csrf_hash)
        .bind(authority.idle_seconds)
        .bind(authority.highlight)
        .bind(id)
        .bind(slug)
        .bind(thread_id)
        .bind(name)
        .bind(&subject)
        .bind(comment.as_str())
        .bind(posted_at)
        .execute(authority.auth_pool)
        .await
        .map_err(staff_post_error)?;
        let ticket = authority
            .ticket_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        sqlx::query("SELECT set_config('board.staff_post_ticket',$1,true)")
            .bind(ticket)
            .execute(&mut *tx)
            .await?;
    }
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
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(slug).bind(thread_id).bind(name).bind(&subject).bind(comment.as_str()).bind(posted_at).execute(&mut *tx).await.map_err(staff_post_error)?;
    }
    if staff.is_none() {
        sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES ($1,$2)")
            .bind(id)
            .bind(&post.deletion_hash)
            .execute(&mut *tx)
            .await?;
    }
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

fn staff_post_error(error: sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("28000")
    {
        StoreError::AuthorizationChanged
    } else {
        StoreError::Database(error)
    }
}

/// Trusted store operation for callers that already own deletion authority.
/// Public password requests must use `delete_with_password_proof` instead.
pub async fn delete_post(pool: &PgPool, slug: &str, id: i64) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    lock_deletion_board(&mut tx, slug).await?;
    delete_post_in(&mut tx, slug, id).await?;
    tx.commit().await?;
    Ok(())
}

/// The proof is the SHA-256 of the stored hash actually verified by the server.
/// Never deserialize this argument from a request. Expensive password work stays
/// outside database locks; current authority is checked inside the mutation.
pub async fn delete_with_password_proof(
    pool: &PgPool,
    slug: &str,
    id: i64,
    proof: [u8; 32],
    file_only: bool,
) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    lock_deletion_board(&mut tx, slug).await?;
    let current: Option<String> = sqlx::query_scalar(crate::read::DELETION_HASH)
        .bind(slug)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    if current.is_none_or(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())) != proof) {
        return Err(StoreError::AuthorizationChanged);
    }
    if file_only {
        post_media::delete_attachment(&mut *tx, slug, id).await?;
    } else {
        delete_post_in(&mut tx, slug, id).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn lock_deletion_board(
    connection: &mut sqlx::PgConnection,
    slug: &str,
) -> Result<(), StoreError> {
    // The next statement must see credential changes committed during this wait,
    // even when the pool's default isolation level is more restrictive.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(slug)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(StoreError::NotFound)?;
    Ok(())
}

async fn delete_post_in(
    connection: &mut sqlx::PgConnection,
    slug: &str,
    id: i64,
) -> Result<(), StoreError> {
    let thread_id: i64 = sqlx::query_scalar(
        "SELECT thread_id FROM content.posts p WHERE board=$1 AND id=$2 AND NOT deleted AND EXISTS (SELECT 1 FROM content.visible_threads t WHERE t.board=p.board AND t.id=p.thread_id) FOR UPDATE",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(StoreError::NotFound)?;
    if id == thread_id {
        sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=$2").bind(slug).bind(id).execute(&mut *connection).await?;
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND thread_id=$2")
            .bind(slug)
            .bind(id)
            .execute(&mut *connection)
            .await?;
    } else {
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(slug)
            .bind(id)
            .execute(&mut *connection)
            .await?;
        sqlx::query(
            "UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=$1 AND id=$2",
        )
        .bind(slug)
        .bind(thread_id)
        .execute(&mut *connection)
        .await?;
    }
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
