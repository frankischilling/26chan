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

#[derive(Clone, Copy)]
pub struct AnonymousPostingContext {
    pub posting: PostingContext,
    pub session: anonymous_session::PostingSession,
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
            spoiler: false,
            keys,
            country_database: None,
            flag: "",
            options: "",
        },
    )
    .await
}

pub struct PostMetadata<'a> {
    pub keys: PostIdentityKeys<'a>,
    pub country_database: Option<&'a board_domain::country::CountryDatabase>,
    /// Public choice, validated against the locked operator-owned board policy.
    pub flag: &'a str,
    /// Raw public options text. Operator-owned board policy decides whether a
    /// dice or fortune request is meaningful after the board row is locked.
    pub options: &'a str,
    /// Raw new-public-post choice, subject to the locked board's SPOILERS policy.
    /// Existing-post changes use the separately authorized staff setter.
    pub spoiler: bool,
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
            anonymous: None,
        },
    )
    .await
}

pub async fn create_post_with_anonymous_session(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&post_media::NewAttachment>,
    context: AnonymousPostingContext,
    metadata: PostMetadata<'_>,
) -> Result<i64, StoreError> {
    create_post_in_context(
        pool,
        slug,
        parent,
        post,
        attachment,
        context.posting,
        PostWriteOptions {
            metadata,
            staff: None,
            anonymous: Some(context.session),
        },
    )
    .await
}

/// Prepared source identity policy; the proof issuer independently checks it.
#[derive(Clone, Copy)]
pub struct StaffPostIdentity<'a> {
    pub capcode: Option<board_domain::capcode::Capcode>,
    pub name_allowed: bool,
    pub administrator: bool,
    pub tripcode_key: Option<&'a board_domain::identity::SecureKey>,
}

/// Server-owned WebAuthn session proof. Request bodies cannot supply this value.
pub struct StaffPostAuthority<'a> {
    pub auth_pool: &'a PgPool,
    pub session_hash: &'a [u8],
    pub csrf_hash: &'a [u8],
    pub ticket_hash: &'a [u8; 32],
    pub idle_seconds: i32,
    pub highlight: bool,
    pub authorized_limits: bool,
    /// Whether the parsed POST name was nonempty, before any normalization.
    pub raw_name_nonempty: bool,
    pub identity: Option<StaffPostIdentity<'a>>,
}

/// Compile-compatible legacy entry point. Without a trusted peer and posting
/// identity key this fails closed; use `create_staff_post_with_context_and_keys`.
pub async fn create_staff_post(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    request_start: DateTime<Utc>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    let tripcode = authority
        .identity
        .and_then(|identity| identity.tripcode_key);
    create_staff_post_with_context_and_keys(
        pool,
        slug,
        parent,
        post,
        PostingContext {
            request_start,
            peer: None,
            op_password_proof: None,
        },
        PostIdentityKeys {
            tripcode,
            poster_id: None,
        },
        authority,
    )
    .await
}

/// Post input and its optional upload, separate from trusted staff authority.
pub struct StaffPostContent<'a> {
    pub post: &'a NewPost,
    pub attachment: Option<&'a post_media::NewAttachment>,
}

/// Trusted staff still contribute private posting history. Both peer and key
/// must come from server-owned state, independent of badge or request fields.
pub async fn create_staff_post_with_context_and_keys(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    context: PostingContext,
    keys: PostIdentityKeys<'_>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    create_staff_post_with_attachment_and_context_and_keys(
        pool,
        slug,
        parent,
        StaffPostContent {
            post,
            attachment: None,
        },
        context,
        keys,
        authority,
    )
    .await
}

/// Attachment authority is bound to the staff proof before the direct insert.
pub async fn create_staff_post_with_attachment_and_context_and_keys(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    content: StaffPostContent<'_>,
    context: PostingContext,
    keys: PostIdentityKeys<'_>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    let StaffPostContent { post, attachment } = content;
    if attachment.is_some() && authority.identity.is_none() {
        return Err(StoreError::Invalid("Source staff identity is required."));
    }
    if authority
        .identity
        .is_some_and(|identity| identity.capcode.is_none())
    {
        return Err(StoreError::Invalid(
            "Ordinary staff posting context is required.",
        ));
    }
    let auth_pool = authority.auth_pool;
    let ticket = authority.ticket_hash;
    let session = authority.session_hash;
    let result = create_post_in_context(
        pool,
        slug,
        parent,
        post,
        attachment,
        context,
        PostWriteOptions {
            metadata: PostMetadata {
                spoiler: false,
                keys,
                country_database: None,
                flag: "",
                options: "",
            },
            staff: Some(authority),
            anonymous: None,
        },
    )
    .await;
    if result.is_err() {
        // The independently committed proof is unused after a failed post.
        // Remove only this request's nonordinary proof. Preserve the original
        // rejection if cleanup fails; the existing 15-second expiry is the
        // fallback and no broader cancellation authority is introduced.
        let _ = sqlx::query("SELECT staff_identity.discard_badged_post_authority($1,$2)")
            .bind(ticket.as_slice())
            .bind(session)
            .execute(auth_pool)
            .await;
    }
    result
}

/// Ordinary staff posts retain public metadata and admission behavior. The
/// independent authority issuer binds every derived value to the saved post.
pub async fn create_ordinary_staff_post(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    post: &NewPost,
    context: PostingContext,
    metadata: PostMetadata<'_>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    create_ordinary_staff_post_with_attachment(
        pool,
        slug,
        parent,
        StaffPostContent {
            post,
            attachment: None,
        },
        context,
        metadata,
        authority,
    )
    .await
}

/// Ordinary attachments retain public identity, password and content admission.
pub async fn create_ordinary_staff_post_with_attachment(
    pool: &PgPool,
    slug: &str,
    parent: i64,
    content: StaffPostContent<'_>,
    context: PostingContext,
    metadata: PostMetadata<'_>,
    authority: StaffPostAuthority<'_>,
) -> Result<i64, StoreError> {
    let StaffPostContent { post, attachment } = content;
    if authority
        .identity
        .is_none_or(|identity| identity.capcode.is_some())
        || context.peer.is_none()
        || metadata.keys.poster_id.is_none()
        || post.deletion_hash.is_empty()
        || post.deletion_hash.len() > 256
        || authority.highlight
    {
        return Err(StoreError::Invalid(
            "Ordinary staff posting context is unavailable.",
        ));
    }
    let auth_pool = authority.auth_pool;
    let ticket = authority.ticket_hash;
    let session = authority.session_hash;
    let result = create_post_in_context(
        pool,
        slug,
        parent,
        post,
        attachment,
        context,
        PostWriteOptions {
            metadata,
            staff: Some(authority),
            anonymous: None,
        },
    )
    .await;
    if result.is_err() {
        // A failed insert or Robot9000 savepoint rollback leaves the separate
        // auth-pool proof unused. Remove only this request's ordinary proof.
        sqlx::query("SELECT staff_identity.discard_ordinary_post_authority($1,$2)")
            .bind(ticket.as_slice())
            .bind(session)
            .execute(auth_pool)
            .await?;
    }
    result
}

pub async fn staff_op_deletion_hash(
    pool: &PgPool,
    slug: &str,
    parent: i64,
) -> Result<Option<String>, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT content.staff_op_deletion_hash($1,$2)")
            .bind(slug)
            .bind(parent)
            .fetch_one(pool)
            .await?,
    )
}

struct PostWriteOptions<'a> {
    metadata: PostMetadata<'a>,
    staff: Option<StaffPostAuthority<'a>>,
    anonymous: Option<anonymous_session::PostingSession>,
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
    let PostWriteOptions {
        metadata,
        staff,
        anonymous,
    } = options;
    if staff.is_some() && attachment.is_none() && metadata.spoiler {
        return Err(StoreError::Invalid("Invalid staff posting metadata."));
    }
    let keys = metadata.keys;
    let ordinary_staff = staff
        .as_ref()
        .and_then(|authority| authority.identity)
        .is_some_and(|identity| identity.capcode.is_none());
    let ordinary = staff.is_none() || ordinary_staff;
    let authorized_staff = staff
        .as_ref()
        .is_some_and(|authority| authority.authorized_limits);
    let prepared_options = board_domain::posting_options::without_sage(metadata.options);
    let preliminary_limits = if staff
        .as_ref()
        .is_some_and(|authority| authority.authorized_limits)
    {
        board_domain::PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS)
            .map_err(|error| StoreError::Invalid(error.0))?
    } else {
        board_domain::PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
    };
    let comment = board_domain::normalize_comment_with_limits(&post.comment, preliminary_limits)
        .map_err(|error| StoreError::Invalid(error.0))?;
    let posted_at = context
        .request_start
        .with_nanosecond(0)
        .ok_or(StoreError::Invalid("Invalid posting timestamp."))?;
    // Public admission requires a server-owned deployment key and trusted peer
    // even on boards that do not display poster IDs. Missing infrastructure is
    // a hard failure; legacy convenience wrappers cannot bypass the gate.
    let posting_actor = {
        let key = keys.poster_id.ok_or_else(|| {
            StoreError::Database(sqlx::Error::Protocol(
                "Posting identity is unavailable.".into(),
            ))
        })?;
        let peer = context.peer.ok_or_else(|| {
            StoreError::Database(sqlx::Error::Protocol(
                "Posting identity is unavailable.".into(),
            ))
        })?;
        key.public_posting_rate_identity(peer)
    };
    let mut tx = pool.begin().await?;
    // The decision must see the preceding actor's commit after waiting on its
    // gate, regardless of the connection's default transaction isolation.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    // Clear attachment intent even when a pooled session previously set it.
    sqlx::query("SELECT set_config('board.staff_attachment_job','',true),set_config('board.staff_attachment_capability','',true),set_config('board.staff_attachment_spoiler','',true),set_config('board.post_image_spoiler','false',true)")
        .execute(&mut *tx)
        .await?;
    crate::posting_cooldown::lock(&mut tx, &posting_actor, parent == 0).await?;
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
    let post_limits = if staff
        .as_ref()
        .is_some_and(|authority| authority.authorized_limits)
    {
        board_domain::PostLimits::authorized(board.max_authorized_comment_chars as usize)
            .map_err(|error| StoreError::Invalid(error.0))?
    } else {
        board_domain::PostLimits::ordinary(board.max_comment_chars as usize)
    };
    let wordfilter_limits = board_domain::WordfilterLimits::for_post(post_limits);
    // Acquire policy, peer and activity locks before OP membership checks.
    // Source filters ordinary staff posts too; cosmetic badges do not confer
    // public identity or bypass authority on an unbadged post.
    let admission = if ordinary || slug == "test" {
        Some(crate::content_admission::begin(&mut tx, slug, context.peer, anonymous).await?)
    } else {
        None
    };
    let special = board_domain::posting_randomizers::request(
        metadata.options,
        board.dice_roll,
        board.fortune_trip,
    )
    .map_err(|error| StoreError::Invalid(error.0))?
    .map(|request| {
        board_domain::posting_randomizers::generate(&request)
            .map_err(|_| StoreError::RandomnessUnavailable)
    })
    .transpose()?;
    let (dice_result, fortune_text, fortune_color) = match special {
        Some(board_domain::posting_randomizers::Outcome::Dice(result)) => {
            (Some(result), None, None)
        }
        Some(board_domain::posting_randomizers::Outcome::Fortune { text, color }) => {
            (None, Some(text), Some(color))
        }
        None => (None, None, None),
    };
    sqlx::query("SELECT set_config('board.dice_result',$1,true),set_config('board.fortune_text',$2,true),set_config('board.fortune_color',$3,true)")
        .bind(dice_result.as_deref().unwrap_or(""))
        .bind(fortune_text.unwrap_or(""))
        .bind(fortune_color.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;
    board.check_attachment_allowed(parent, attachment.is_some())?;
    let flag = if metadata.flag == "0" {
        ""
    } else {
        metadata.flag
    };
    if !flag.is_empty()
        && (!board.board_flags.iter().any(|enabled| enabled == flag)
            || board_domain::board_flags::flag(&board.board_flag_type, flag).is_none())
    {
        return Err(StoreError::Invalid("Invalid board flag."));
    }
    let country = if ordinary && board.country_flags && flag.is_empty() {
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
    // Badged staff now supply a peer for private cooldown accounting, but must
    // not thereby acquire public OP membership or direct secret-table access.
    let peer = context
        .peer
        .filter(|_| ordinary)
        .map(|peer| peer.to_canonical().to_string());
    let staff_op: Option<(bool, Option<DateTime<Utc>>)> = if ordinary_staff && parent > 0 {
        sqlx::query_as("SELECT * FROM content.staff_op_context($1,$2,$3)")
            .bind(slug)
            .bind(parent)
            .bind(&peer)
            .fetch_optional(&mut *tx)
            .await?
    } else {
        None
    };
    let own_reply = if ordinary_staff {
        staff_op.as_ref().is_some_and(|(own, _)| *own)
    } else if parent > 0 {
        if let Some(peer) = &peer {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=$1 AND peer=$2::text::inet)")
                .bind(parent).bind(peer).fetch_one(&mut *tx).await?
        } else {
            false
        }
    } else {
        false
    };
    let password_matches = if ordinary && board.op_markup && parent > 0 {
        if let Some(proof) = context.op_password_proof {
            let hash: Option<String> = if ordinary_staff {
                sqlx::query_scalar("SELECT content.staff_op_deletion_hash($1,$2)")
                    .bind(slug)
                    .bind(parent)
                    .fetch_one(&mut *tx)
                    .await?
            } else {
                sqlx::query_scalar(crate::read::OP_DELETION_HASH)
                    .bind(slug)
                    .bind(parent)
                    .fetch_optional(&mut *tx)
                    .await?
            };
            hash.is_some_and(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())) == proof)
        } else {
            false
        }
    } else {
        false
    };
    let session_matches = if staff.is_none() && board.op_markup && parent > 0 {
        if let Some(session) = anonymous {
            anonymous_session::locked_post_proof(&mut tx, &session.fingerprints.token, slug, parent)
                .await?
                .is_some()
        } else {
            false
        }
    } else {
        false
    };
    let op_markup =
        board.op_markup && (parent == 0 || own_reply || password_matches || session_matches);
    // Source clears identity before required-subject and final content checks.
    // Retain the raw input bounds even when these fields will be discarded.
    let (post_name, post_subject) = if board.forced_anon {
        board_domain::validate_post_with_limits(
            &post.name,
            &post.subject,
            &comment,
            post_limits,
            true,
        )
        .map_err(|error| StoreError::Invalid(error.0))?;
        (
            if staff
                .as_ref()
                .and_then(|authority| authority.identity)
                .is_some_and(|identity| identity.administrator)
            {
                post.name.as_str()
            } else {
                "Anonymous"
            },
            "",
        )
    } else {
        (post.name.as_str(), post.subject.as_str())
    };
    let identity = if let Some(source) = staff.as_ref().and_then(|authority| authority.identity) {
        let prepared = board_domain::identity::prepare_for_board_with_limits(
            post_name,
            keys.tripcode,
            board.comment_spacing(),
            board.strip_tripcode,
            post_limits,
        )
        .map_err(|error| StoreError::Invalid(error.0))?;
        // Source checks the finished name/trip bound before masking the name.
        if source.name_allowed {
            prepared
        } else {
            board_domain::identity::Identity {
                name: "Anonymous".into(),
                trip: None,
            }
        }
    } else if staff.is_some() {
        // Legacy proofs and private discussion never publish a raw trip secret.
        let display = post_name.split('#').next().unwrap_or("").trim();
        if board_domain::source_html_entities(display).len()
            > board_domain::identity::MAX_DISPLAY_NAME_BYTES
        {
            return Err(StoreError::Invalid("Name or subject is too long."));
        }
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
        board_domain::identity::prepare_for_board(
            post_name,
            keys.tripcode,
            board.comment_spacing(),
            board.strip_tripcode,
        )
        .map_err(|error| StoreError::Invalid(error.0))?
    };
    let content = board_domain::prepare_post_content_input_with_limits(
        post_name,
        post_subject,
        &comment,
        post_limits,
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
    // Invalid, closed or full targets cannot create filter effects or fake
    // success. The board lock keeps this snapshot valid until the later insert.
    let reply_target = if parent > 0 {
        let thread: Thread = sqlx::query_as("SELECT * FROM content.threads WHERE board=$1 AND id=$2 AND NOT deleted AND EXISTS(SELECT 1 FROM content.visible_threads WHERE board=$1 AND id=$2) FOR UPDATE")
            .bind(slug).bind(parent).fetch_optional(&mut *tx).await?.ok_or(StoreError::NotFound)?;
        if thread.archived_at.is_some()
            || (!authorized_staff
                && (thread.closed
                    || (thread.reply_count >= board.reply_limit
                        && !(thread.sticky && thread.undead && board.reply_limit > 1))))
        {
            return Err(StoreError::Conflict(
                "This thread is closed or has reached its reply limit.",
            ));
        }
        Some(thread)
    } else {
        None
    };
    // Source checks active OP quota after bans and before content filters,
    // duplicate admission and cooldowns. Staff must first obtain the bound
    // authority below; a cosmetic identity must never authorize this check.
    if parent == 0 && staff.is_none() {
        crate::thread_quota::check(
            &mut tx,
            &posting_actor,
            slug,
            posted_at.timestamp(),
            anonymous,
        )
        .await?;
    }
    let mut autosage_proof = None;
    if let Some(admission) = admission {
        let filename = if let Some(attachment) = attachment {
            sqlx::query_scalar("SELECT content.attachment_upload_filename($1,$2)")
                .bind(&attachment.upload.id)
                .bind(&attachment.upload.capability)
                .fetch_one(&mut *tx)
                .await
                .map_err(post_media::scoped_error)?
        } else {
            String::new()
        };
        let evaluated = admission
            .evaluate(crate::content_admission::Input {
                board: slug.into(),
                parent,
                // The source hook receives escaped display text and the public
                // trip span after hashing, never the private trip password.
                name: match &identity.trip {
                    Some(trip) => format!(
                        "{}</span> <span class=\"postertrip\">{trip}",
                        board_domain::source_html_entities(&identity.name)
                    ),
                    None => board_domain::source_html_entities(&identity.name),
                },
                // Admit only a derived legacy hash. The source leaves its raw
                // second field in $trip when suppression skips hashing; this
                // backend excludes that secret from admission and logging.
                legacy_trip: identity
                    .trip
                    .as_deref()
                    .filter(|trip| !trip.starts_with("!!"))
                    .and_then(|trip| trip.strip_prefix('!'))
                    .unwrap_or("")
                    .into(),
                subject: board_domain::source_html_entities(content.subject()),
                comment: board_domain::source_html_entities(content.comment()),
                filename,
            })
            .await?;
        evaluated.record(&mut tx).await?;
        if let Some(message) = evaluated.rejection() {
            tx.commit().await?;
            return Err(StoreError::ContentRejected(message));
        }
        if matches!(
            evaluated.decision,
            board_domain::content_admission::Decision::Reject { quiet: true, .. }
        ) {
            let post = crate::content_admission::quiet_post(&mut tx, slug, parent).await?;
            tx.commit().await?;
            return Err(StoreError::ContentQuiet { post });
        }
        autosage_proof = evaluated.autosage_proof();
        if let Some(message) = evaluated.trip_rejection() {
            // Later admission failures share the posting transaction. Do not
            // retain a preceding log/autosage hit from an incomplete post.
            return Err(StoreError::ContentRejected(message.into()));
        }
    }
    let board_domain::PreparedPostContent { comment, subject } = content
        .finish()
        .map_err(|error| StoreError::Invalid(error.0))?;
    let mut wordfiltered = if board.word_filter_enabled {
        use board_domain::wordfilter::{LeetRolls, Profile};
        let profile = match board.word_filter_profile {
            0 => Profile::Global,
            1 => Profile::Basic,
            2 => Profile::Asp,
            3 => Profile::Video,
            4 => Profile::Test,
            _ => return Err(StoreError::Invalid("Wordfilter policy is unavailable.")),
        };
        let rolls = if profile == Profile::Test {
            Some(LeetRolls::generate().map_err(|_| StoreError::RandomnessUnavailable)?)
        } else {
            None
        };
        Some(
            board_domain::wordfiltered_comment::prepare_with_limits(
                &comment,
                board_domain::comment_markup::MarkupPolicy {
                    spoilers: board.comment_spoiler_cleanup,
                    code: board.comment_code_spacing,
                    sjis: board.comment_sjis_spacing,
                    op: op_markup,
                },
                profile,
                rolls,
                post_limits,
            )
            .map_err(|error| StoreError::Invalid(error.0))?,
        )
    } else {
        None
    };
    if let Some(prepared) = &mut wordfiltered {
        prepared.freeze_format(slug);
    }
    let wordfilter_payload = wordfiltered
        .as_ref()
        .map(board_domain::wordfiltered_comment::PreparedComment::encode)
        .transpose()
        .map_err(|error| StoreError::Invalid(error.0))?;
    let formatted = wordfiltered
        .as_ref()
        .map(|prepared| board_domain::filtered_formatting::lines(prepared, slug));
    let wordfilter_search = formatted
        .as_ref()
        .map(|lines| board_domain::formatting::plain_text(lines));
    if wordfilter_search
        .as_ref()
        .is_some_and(|text| text.len() > wordfilter_limits.stored_bytes())
    {
        return Err(StoreError::Invalid("Wordfilter output is too large."));
    }
    let comment = formatted.as_ref().map_or(comment, |lines| {
        board_domain::filtered_formatting::source_projection(lines)
    });
    if comment.len() > wordfilter_limits.output_bytes() {
        return Err(StoreError::Invalid("Wordfilter output is too large."));
    }
    let encoded_payload = wordfilter_payload
        .as_ref()
        .map_or_else(String::new, |bytes| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        });
    sqlx::query("SELECT set_config('board.wordfilter_payload',$1,true),set_config('board.wordfilter_search',$2,true)")
        .bind(encoded_payload)
        .bind(wordfilter_search.as_deref().unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    // Staff admission waits for the independent proof issuer below. It
    // authoritatively selects named/meta janitor ordinary timers before the
    // separate five-second staff check.
    if staff.is_none() {
        crate::posting_cooldown::check(
            &mut tx,
            &posting_actor,
            slug,
            parent,
            attachment.is_some(),
            posted_at.timestamp(),
        )
        .await?;
    }
    // Keep the locked policy outside the savepoint. A rejected post rolls back
    // rollover, counters, attachments and secrets, then persists only its mute.
    let robot_applies = match staff.as_ref() {
        None => {
            board_domain::robot9000::applies_to_post(board.robot9000, None, metadata.options, false)
        }
        Some(authority) => authority.identity.is_some_and(|identity| {
            board_domain::robot9000::applies_to_post(
                board.robot9000,
                identity.capcode,
                &prepared_options,
                true,
            )
        }),
    };
    let robot_actor = if robot_applies {
        let key = keys
            .poster_id
            .ok_or(StoreError::Invalid("Robot9000 identity is unavailable."))?;
        let peer = context
            .peer
            .ok_or(StoreError::Invalid("Robot9000 identity is unavailable."))?;
        Some(
            key.robot9000_fingerprint(slug, peer)
                .map_err(|error| StoreError::Invalid(error.0))?,
        )
    } else {
        None
    };
    if robot_actor.is_some() {
        sqlx::query("SAVEPOINT robot9000_post")
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("SELECT set_config('board.content_autosage',$1,true),set_config('board.content_admission_revision',$2,true),set_config('board.content_admission_peer',$3,true)")
        .bind(autosage_proof.map(|(rule,_)| rule.to_string()).unwrap_or_default())
        .bind(autosage_proof.map(|(_,revision)| revision.to_string()).unwrap_or_default())
        .bind(context.peer.map(|peer| peer.to_canonical().to_string()).unwrap_or_default())
        .execute(&mut *tx).await?;
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await?;
    let thread_id = if parent == 0 {
        if staff.is_none() {
            crate::archives::make_room(&mut tx, &board).await?;
            sqlx::query(
                "INSERT INTO content.threads(id,board,created_at,modified_at) VALUES ($1,$2,$3,$3)",
            )
            .bind(id)
            .bind(slug)
            .bind(posted_at)
            .execute(&mut *tx)
            .await?;
        }
        id
    } else {
        parent
    };
    let poster_id = if ordinary && board.user_ids {
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
    sqlx::query(
        "SELECT set_config('board.poster_id', $1, true),set_config('board.post_sage', $2, true)",
    )
    .bind(poster_id.as_deref().unwrap_or(""))
    .bind(post.sage.to_string())
    .execute(&mut *tx)
    .await?;
    let count_context = keys
        .poster_id
        .zip(context.peer.filter(|_| ordinary))
        .map(|(key, peer)| key.count_context(slug, thread_id, peer))
        .transpose()
        .map_err(|error| StoreError::Invalid(error.0))?;
    sqlx::query("SELECT set_config('board.poster_fingerprint', $1, true),set_config('board.poster_epoch', $2, true)")
        .bind(count_context.as_ref().map_or("", |value| value.fingerprint.as_str()))
        .bind(count_context.as_ref().map_or("", |value| value.epoch.as_str()))
        .execute(&mut *tx).await?;
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
    let spoiler = board.comment_spoiler_cleanup
        && (metadata.spoiler || attachment.is_some_and(|file| file.spoiler));
    if staff.is_some() {
        sqlx::query("SELECT set_config('board.staff_attachment_job',$1,true),set_config('board.staff_attachment_capability',$2,true),set_config('board.staff_attachment_spoiler',$3,true)")
            .bind(attachment.map_or("", |file| file.upload.id.as_str()))
            .bind(attachment.map_or("", |file| file.upload.capability.as_str()))
            .bind(match attachment {
                Some(_) if spoiler => "true",
                Some(_) => "false",
                None => "",
            })
            .execute(&mut *tx)
            .await?;
    }
    let ordinary_timers = if let Some(authority) = &staff {
        let ordinary_timers: bool = if ordinary_staff {
            let proof = context
                .op_password_proof
                .map(|proof| {
                    proof
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                })
                .unwrap_or_default();
            sqlx::query("SELECT set_config('board.peer',$1,true),set_config('board.deletion_hash',$2,true),set_config('board.op_password_proof',$3,true),set_config('board.staff_post_options',$4,true)")
                .bind(&peer).bind(&post.deletion_hash).bind(proof).bind(&prepared_options)
                .execute(&mut *tx).await?;
            let bound_context: sqlx::types::JsonValue =
                sqlx::query_scalar("SELECT content.staff_ordinary_context()")
                    .fetch_one(&mut *tx)
                    .await?;
            let issuer = if attachment.is_some() {
                "SELECT staff_identity.issue_ordinary_attachment_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)"
            } else {
                "SELECT staff_identity.issue_ordinary_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"
            };
            let mut proof = sqlx::query_scalar(issuer)
                .bind(authority.ticket_hash.as_slice())
                .bind(authority.session_hash)
                .bind(authority.csrf_hash)
                .bind(authority.idle_seconds)
                .bind(id)
                .bind(slug)
                .bind(thread_id)
                .bind(name)
                .bind(&subject)
                .bind(comment.as_str())
                .bind(posted_at)
                .bind(authority.authorized_limits)
                .bind(post_limits.comment_chars() as i32)
                .bind(wordfilter_payload.as_deref())
                .bind(wordfilter_search.as_deref())
                .bind(&prepared_options)
                .bind(identity.trip.as_deref())
                .bind(authority.identity.expect("ordinary identity").name_allowed)
                .bind(bound_context)
                .bind(authority.raw_name_nonempty);
            if let Some(attachment) = attachment {
                proof = proof
                    .bind(&attachment.upload.id)
                    .bind(&attachment.upload.capability)
                    .bind(spoiler);
            }
            proof
                .fetch_one(authority.auth_pool)
                .await
                .map_err(staff_post_error)?
        } else {
            let issuer = if attachment.is_some() {
                "SELECT staff_identity.issue_source_attachment_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)"
            } else if authority.identity.is_some() {
                "SELECT staff_identity.issue_source_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)"
            } else {
                "SELECT staff_identity.issue_limited_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)"
            };
            let mut proof = sqlx::query_scalar(issuer)
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
                .bind(authority.authorized_limits)
                .bind(post_limits.comment_chars() as i32)
                .bind(wordfilter_payload.as_deref())
                .bind(wordfilter_search.as_deref());
            if let Some(source) = authority.identity {
                proof = proof
                    .bind(
                        source
                            .capcode
                            .expect("validated badge identity")
                            .source_option(),
                    )
                    .bind(identity.trip.as_deref())
                    .bind(source.name_allowed);
            }
            proof = proof.bind(authority.raw_name_nonempty);
            if let Some(attachment) = attachment {
                proof = proof
                    .bind(&attachment.upload.id)
                    .bind(&attachment.upload.capability)
                    .bind(spoiler);
            }
            proof
                .fetch_one(authority.auth_pool)
                .await
                .map_err(staff_post_error)?
        };
        let ticket = authority
            .ticket_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        sqlx::query("SELECT set_config('board.staff_post_ticket',$1,true),set_config('board.staff_raw_name_nonempty',$2,true)")
            .bind(ticket)
            .bind(authority.raw_name_nonempty.to_string())
            .execute(&mut *tx)
            .await?;
        // Actor/board locks and READ COMMITTED are already held. A validated
        // authority, not capcode or client fields, selects this admission path.
        // All staff OPs share the IP quota, including badged and private-board
        // posts. Check before timers and rollover can remove counted rows.
        if parent == 0 {
            crate::thread_quota::check(
                &mut tx,
                &posting_actor,
                slug,
                posted_at.timestamp(),
                anonymous,
            )
            .await?;
        }
        if ordinary_timers {
            crate::posting_cooldown::check_janitor(
                &mut tx,
                &posting_actor,
                slug,
                parent,
                attachment.is_some(),
                posted_at.timestamp(),
            )
            .await?;
        }
        crate::posting_cooldown::check_staff(&mut tx, &posting_actor, slug, posted_at.timestamp())
            .await?;
        if parent == 0 {
            // Check before rollover: deleting/archiving the newest post here
            // must not erase the history used to admit this staff attempt.
            crate::archives::make_room(&mut tx, &board).await?;
            sqlx::query(
                "INSERT INTO content.threads(id,board,created_at,modified_at) VALUES ($1,$2,$3,$3)",
            )
            .bind(id)
            .bind(slug)
            .bind(posted_at)
            .execute(&mut *tx)
            .await?;
        }
        ordinary_timers
    } else {
        true
    };
    if parent > 0 {
        let thread = reply_target.expect("validated locked reply target");
        // imgboard.php keeps the newest STICKY_CAP - 1 existing replies before
        // inserting into an undead sticky. Imported source boards use the same
        // 1,000-reply bound as STICKY_CAP; synthetic boards may set a smaller
        // reply_limit for the identical policy. The locked board and thread are
        // held through the commit, so this window cannot change under us.
        //
        // This executes after admission/proof/cooldowns and within the Robot9000
        // savepoint. Every deletion (including private proof retirement) rolls
        // back if media insertion or Robot9000 rejects the new reply.
        if thread.sticky && thread.undead && board.reply_limit > 1 {
            sqlx::query("SELECT set_config('board.sticky_prune_thread',$1,true)")
                .bind(parent.to_string())
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "WITH retained AS MATERIALIZED (
                    SELECT id FROM content.posts
                    WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted
                    ORDER BY id DESC LIMIT $3
                 ), boundary AS (SELECT min(id) AS first_retained FROM retained)
                 UPDATE content.posts p SET deleted=true FROM boundary b
                 WHERE p.board=$1 AND p.thread_id=$2 AND p.id<>$2 AND NOT p.deleted
                   AND b.first_retained IS NOT NULL AND p.id < b.first_retained",
            )
            .bind(slug)
            .bind(parent)
            .bind(i64::from(board.reply_limit - 1))
            .execute(&mut *tx)
            .await?;
            sqlx::query("SELECT set_config('board.sticky_prune_thread','',true)")
                .execute(&mut *tx)
                .await?;
        }
        // Count under the same board lock as posting/deletion. The incoming
        // row is not inserted yet; the source's decision includes that reply.
        let (replies, op_created): (i64, DateTime<Utc>) = sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted),created_at FROM content.posts WHERE board=$1 AND id=$2 AND NOT deleted")
            .bind(slug).bind(parent).fetch_one(&mut *tx).await?;
        let mut self_sage = false;
        // Public posts and issuer-authenticated named/meta janitors use the
        // source OP self-bump timers; OP membership is independent of this gate.
        if ordinary_timers && board.op_bump_limit {
            let (bump_own_reply, latest) = crate::op_bump::context(
                &mut tx,
                &posting_actor,
                slug,
                parent,
                staff.is_some(),
                context.peer,
                own_reply,
            )
            .await?;
            self_sage = board_domain::op_bump::limited(
                bump_own_reply,
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
        // Source sticky windows use the surviving count. All other threads
        // retain their existing cumulative cached increment behavior.
        let visible_after = i32::try_from(replies + 1)
            .map_err(|_| StoreError::Invalid("Thread reply count exceeds storage limits."))?;
        sqlx::query("UPDATE content.threads SET reply_count=CASE WHEN $6 THEN $4 ELSE reply_count+1 END, modified_at=$3, bumped_at=CASE WHEN $2 THEN clock_timestamp() ELSE bumped_at END WHERE board=$5 AND id=$1")
            .bind(parent)
            .bind(bump)
            .bind(posted_at)
            .bind(visible_after)
            .bind(slug)
            .bind(thread.sticky && thread.undead && board.reply_limit > 1)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("SELECT set_config('board.post_image_spoiler',$1,true)")
        .bind(if staff.is_none() && spoiler {
            "true"
        } else {
            "false"
        })
        .execute(&mut *tx)
        .await?;
    // The insert-only trigger registers history inside the post savepoint.
    // A Robot9000 rejection rolls it back together with the post and actions.
    crate::posting_cooldown::set_insert_actor(&mut tx, &posting_actor).await?;
    if let Some(attachment) = attachment.filter(|_| staff.is_none()) {
        sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(id)
            .bind(slug)
            .bind(thread_id)
            .bind(name)
            .bind(&subject)
            .bind(comment.as_str())
            .bind(&attachment.upload.id)
            .bind(&attachment.upload.capability)
            .bind(spoiler)
            .bind(posted_at)
            .execute(&mut *tx)
            .await
            .map_err(post_media::scoped_error)?;
    } else {
        // Staff inserts run as board_staff so the proof trigger checks them.
        // The private AFTER trigger attaches media using the consumed proof.
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(id)
            .bind(slug)
            .bind(thread_id)
            .bind(name)
            .bind(&subject)
            .bind(comment.as_str())
            .bind(posted_at)
            .execute(&mut *tx)
            .await
            .map_err(|error| {
                if attachment.is_some() {
                    match staff_post_error(error) {
                        StoreError::Database(error) => post_media::scoped_error(error),
                        error => error,
                    }
                } else {
                    staff_post_error(error)
                }
            })?;
    }
    if staff.is_none() {
        sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES ($1,$2)")
            .bind(id)
            .bind(&post.deletion_hash)
            .execute(&mut *tx)
            .await?;
    }
    if !ordinary_staff && parent == 0 {
        if let Some(peer) = peer {
            sqlx::query(
                "INSERT INTO post_secrets.op_peers(thread_id,peer) VALUES($1,$2::text::inet)",
            )
            .bind(id)
            .bind(peer)
            .execute(&mut *tx)
            .await?;
        }
    } else if !ordinary_staff && own_reply {
        sqlx::query("INSERT INTO post_secrets.op_replies(post_id,thread_id) VALUES($1,$2)")
            .bind(id)
            .bind(parent)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(actor) = robot_actor {
        let prepared = if wordfiltered.is_some() {
            board_domain::robot9000::prepare(&comment)
        } else {
            board_domain::robot9000::prepare_post(
                &comment,
                board_domain::comment_markup::MarkupPolicy {
                    spoilers: board.comment_spoiler_cleanup,
                    code: board.comment_code_spacing,
                    sjis: board.comment_sjis_spacing,
                    op: op_markup,
                },
                slug,
            )
        }
        .map_err(StoreError::Invalid)?;
        let now = Utc::now()
            .with_nanosecond(0)
            .ok_or(StoreError::Invalid("Invalid posting timestamp."))?;
        let decision = crate::robot9000::check(&mut tx, slug, &actor, &prepared, now).await?;
        if let crate::robot9000::Decision::Reject(message) = &decision {
            sqlx::query("ROLLBACK TO SAVEPOINT robot9000_post")
                .execute(&mut *tx)
                .await?;
            // The pre-savepoint board lock prevents another writer changing
            // the decision between rollback and the state-only commit.
            if crate::robot9000::check(&mut tx, slug, &actor, &prepared, now).await? != decision {
                return Err(StoreError::Database(sqlx::Error::Protocol(
                    "Robot9000 decision changed under the board lock.".into(),
                )));
            }
            tx.commit().await?;
            return Err(StoreError::Robot9000Rejected(message.clone()));
        }
    }
    if let Some(session) = anonymous {
        anonymous_session::record_post(&mut tx, session, slug, id).await?;
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
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
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
    batch: &mut PublicDeletionBatch,
    id: i64,
    proof: [u8; 32],
    file_only: bool,
) -> Result<(), StoreError> {
    let mut tx = batch.begin(pool).await?;
    let slug = batch.slug();
    let context = batch.context();
    lock_deletion_board(&mut tx, slug).await?;
    let eligibility = public_deletion::eligibility(&mut tx, slug, id).await?;
    eligibility.before_authority(context.request_start)?;
    let current: Option<String> = sqlx::query_scalar(crate::read::DELETION_HASH)
        .bind(slug)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    if current.is_none_or(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())) != proof) {
        return Err(StoreError::AuthorizationChanged);
    }
    eligibility.after_authority(&mut tx, context).await?;
    if file_only {
        post_media::delete_attachment(&mut *tx, slug, id).await?;
    } else {
        delete_post_in(&mut tx, slug, id).await?;
    }
    batch.commit(tx).await
}

/// Compatibility name; all authority and quota context belongs to the batch.
pub async fn delete_with_password_proof_context(
    pool: &PgPool,
    batch: &mut PublicDeletionBatch,
    id: i64,
    proof: [u8; 32],
    file_only: bool,
) -> Result<(), StoreError> {
    delete_with_password_proof(pool, batch, id, proof, file_only).await
}

/// Recheck and lock the private session membership under the board mutation
/// lock. Revocation or password rotation during the earlier read fails closed.
pub async fn delete_with_anonymous_proof(
    pool: &PgPool,
    batch: &mut PublicDeletionBatch,
    id: i64,
    token: [u8; 32],
    proof: [u8; 32],
    file_only: bool,
) -> Result<(), StoreError> {
    let context = batch.context();
    if context
        .session
        .is_some_and(|session| session.fingerprints.token != token)
    {
        return Err(StoreError::AuthorizationChanged);
    }
    let mut tx = batch.begin(pool).await?;
    let slug = batch.slug();
    lock_deletion_board(&mut tx, slug).await?;
    let eligibility = public_deletion::eligibility(&mut tx, slug, id).await?;
    eligibility.before_authority(context.request_start)?;
    if anonymous_session::locked_post_proof(&mut tx, &token, slug, id).await? != Some(proof) {
        return Err(StoreError::AuthorizationChanged);
    }
    eligibility.after_authority(&mut tx, context).await?;
    if file_only {
        post_media::delete_attachment(&mut *tx, slug, id).await?;
    } else {
        delete_post_in(&mut tx, slug, id).await?;
    }
    batch.commit(tx).await
}

/// Compatibility name; all authority and quota context belongs to the batch.
pub async fn delete_with_anonymous_proof_context(
    pool: &PgPool,
    batch: &mut PublicDeletionBatch,
    id: i64,
    token: [u8; 32],
    proof: [u8; 32],
    file_only: bool,
) -> Result<(), StoreError> {
    delete_with_anonymous_proof(pool, batch, id, token, proof, file_only).await
}

async fn lock_deletion_board(
    connection: &mut sqlx::PgConnection,
    slug: &str,
) -> Result<(), StoreError> {
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

/// Staff-only legacy report admission using a trusted transport IP identity.
/// A public runtime pool cannot execute this IP-only database path.
pub async fn report(
    pool: &PgPool,
    slug: &str,
    id: i64,
    reason: &str,
    identity: &board_domain::poster_id::PublicReportRateIdentity,
) -> Result<(), StoreError> {
    validate_report_reason(reason)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    crate::report_admission::admit_on(&mut tx, slug, id, reason, identity).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn report_with_anonymous_session(
    pool: &PgPool,
    slug: &str,
    id: i64,
    reason: &str,
    identity: &board_domain::poster_id::PublicReportRateIdentity,
    session: anonymous_session::PostingSession,
) -> Result<(), StoreError> {
    validate_report_reason(reason)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    crate::report_admission::admit_with_session_on(&mut tx, slug, id, reason, identity, session)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Opt-in category admission with the same mandatory trusted actor and
/// anonymous-session boundary as the public free-text path. No caller-provided
/// title, kind, weight, filtering state, or effective priority is accepted.
pub async fn report_categorical_with_anonymous_session(
    pool: &PgPool,
    slug: &str,
    id: i64,
    category_id: i64,
    expected_revision: i64,
    identity: &board_domain::poster_id::PublicReportRateIdentity,
    session: anonymous_session::PostingSession,
) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    crate::report_admission::admit_categorical_with_session_on(
        &mut tx,
        slug,
        id,
        category_id,
        expected_revision,
        identity,
        session,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

fn validate_report_reason(reason: &str) -> Result<(), StoreError> {
    if reason.trim().is_empty() || reason.len() > 1000 || reason.contains('\0') {
        return Err(StoreError::Invalid(
            "Report reason must contain 1 to 1000 bytes.",
        ));
    }
    Ok(())
}
