//! Role-split destructive text transfers. No private author evidence leaves SQL.
//! Callers must release any lock-bearing authentication guard before using this
//! module's independently committed calls on the auth and staff pools.

use board_domain::saved_presentation::{
    FrozenPresentation, PresentationError, PriorReplacement, RewriteBudget, SavedCommentRef,
};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

const BODY_LIMIT: usize = 64_064_000;
const WORK_LIMIT: usize = 268_435_456;
static FORMATTERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Existing hashed session and CSRF values, derived by the trusted HTTP layer.
/// Deliberately not Debug or serializable.
pub struct Credentials {
    pub session_hash: [u8; 32],
    pub csrf_hash: [u8; 32],
    pub idle_seconds: i32,
}

/// Server-random opaque capability. Never put this value in an HTTP form/log.
#[derive(Clone)]
pub struct Ticket([u8; 32]);

impl Ticket {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransferError {
    #[error("Transfer authorization is unavailable.")]
    Forbidden,
    #[error("Recent authentication is required.")]
    Recent,
    #[error("Transfer source or board is unavailable.")]
    NotFound,
    #[error("Invalid transfer request.")]
    Invalid,
    #[error("Transfer is busy. Nothing was changed by this attempt.")]
    Busy,
    #[error("The prepared transfer has expired.")]
    Expired,
    #[error("The source or destination changed. Prepare a new transfer.")]
    Drift,
    #[error("This saved presentation cannot be transferred losslessly.")]
    Format,
    #[error("Transfer exceeds the available bounded capacity.")]
    Capacity,
    #[error("Transfer database is unavailable; check the result before retrying.")]
    Database,
}

fn database_error(error: sqlx::Error) -> TransferError {
    let code = error.as_database_error().and_then(|error| error.code());
    match code.as_deref() {
        Some("P131A" | "42501") => TransferError::Forbidden,
        Some("P131Q") => TransferError::Recent,
        Some("P131N") => TransferError::NotFound,
        Some("P131I" | "22023") => TransferError::Invalid,
        Some("P131E") => TransferError::Expired,
        Some("P131C" | "23505") => TransferError::Drift,
        Some("P131F") => TransferError::Format,
        Some("P131L") => TransferError::Capacity,
        Some("55P03" | "40P01" | "40001") => TransferError::Busy,
        _ => TransferError::Database,
    }
}

fn presentation_error(error: PresentationError) -> TransferError {
    match error {
        PresentationError::LimitExceeded | PresentationError::WorkLimitExceeded => {
            TransferError::Capacity
        }
        _ => TransferError::Format,
    }
}

#[derive(sqlx::FromRow)]
struct PreparedRow {
    request_id: String,
    expires_at: DateTime<Utc>,
    source_fingerprint: Vec<u8>,
    source_board: String,
    source_thread: i64,
    destination_board: String,
    destination_thread: i64,
    ordinal: i32,
    source_post_id: i64,
    destination_post_id: i64,
    comment: String,
    comment_format: i16,
    staff_authorized_limits: bool,
    wordfilter_payload: Option<Vec<u8>>,
    saved_presentation: Option<Vec<u8>>,
}

/// Only prepare can construct this complete, immutable database snapshot/map.
/// Arc permits bounded blocking formatting without cloning the source bodies.
pub struct PreparedTransfer {
    ticket: Ticket,
    rows: Arc<Vec<PreparedRow>>,
}

impl PreparedTransfer {
    pub fn request_id(&self) -> &str {
        &self.rows[0].request_id
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.rows[0].expires_at
    }

    pub fn post_count(&self) -> usize {
        self.rows.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostMapping {
    pub source_post_id: i64,
    pub destination_post_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferResult {
    pub request_id: String,
    pub source_board: String,
    pub source_thread: i64,
    pub destination_board: String,
    pub destination_thread: i64,
    pub post_count: i64,
    pub audit_id: i64,
    pub posts: Vec<PostMapping>,
}

#[derive(sqlx::FromRow)]
struct ResultRow {
    request_id: String,
    source_board: String,
    source_thread: i64,
    destination_board: String,
    destination_thread: i64,
    post_count: i64,
    audit_id: i64,
    source_post_id: i64,
    destination_post_id: i64,
}

fn result_rows(rows: Vec<ResultRow>) -> Result<Option<TransferResult>, TransferError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    if !(1..=1001).contains(&first.post_count)
        || usize::try_from(first.post_count).ok() != Some(rows.len())
        || first.source_post_id != first.source_thread
        || first.destination_post_id != first.destination_thread
        || rows
            .iter()
            .map(|row| row.source_post_id)
            .collect::<BTreeSet<_>>()
            .len()
            != rows.len()
        || rows
            .iter()
            .map(|row| row.destination_post_id)
            .collect::<BTreeSet<_>>()
            .len()
            != rows.len()
        || rows.iter().any(|row| {
            row.request_id != first.request_id
                || row.source_board != first.source_board
                || row.source_thread != first.source_thread
                || row.destination_board != first.destination_board
                || row.destination_thread != first.destination_thread
                || row.post_count != first.post_count
                || row.audit_id != first.audit_id
                || row.source_post_id <= 0
                || row.destination_post_id <= 0
        })
    {
        return Err(TransferError::Database);
    }
    Ok(Some(TransferResult {
        request_id: first.request_id.clone(),
        source_board: first.source_board.clone(),
        source_thread: first.source_thread,
        destination_board: first.destination_board.clone(),
        destination_thread: first.destination_thread,
        post_count: first.post_count,
        audit_id: first.audit_id,
        posts: rows
            .into_iter()
            .map(|row| PostMapping {
                source_post_id: row.source_post_id,
                destination_post_id: row.destination_post_id,
            })
            .collect(),
    }))
}

async fn begin(pool: &PgPool) -> Result<Transaction<'_, Postgres>, TransferError> {
    let mut tx = pool.begin().await.map_err(database_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    Ok(tx)
}

pub async fn ready(pool: &PgPool) -> Result<bool, TransferError> {
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(pool)
        .await
        .map_err(database_error)?;
    let query = match role.as_str() {
        "board_auth" => "SELECT staff_identity.text_thread_transfer_ready()",
        "board_staff" => "SELECT content.text_thread_transfer_ready()",
        _ => return Err(TransferError::Forbidden),
    };
    sqlx::query_scalar(query)
        .fetch_one(pool)
        .await
        .map_err(database_error)
}

pub async fn prepare(
    auth_pool: &PgPool,
    credentials: &Credentials,
    ticket: &Ticket,
    source_board: &str,
    source_thread: i64,
    destination_board: &str,
) -> Result<PreparedTransfer, TransferError> {
    let mut tx = begin(auth_pool).await?;
    let rows: Vec<PreparedRow> = sqlx::query_as(
        "SELECT * FROM staff_identity.prepare_text_thread_transfer($1,$2,$3,$4,$5,$6,$7,1::smallint)",
    )
    .bind(ticket.0.as_slice())
    .bind(credentials.session_hash.as_slice())
    .bind(credentials.csrf_hash.as_slice())
    .bind(credentials.idle_seconds)
    .bind(source_board)
    .bind(source_thread)
    .bind(destination_board)
    .fetch_all(&mut *tx)
    .await
    .map_err(database_error)?;
    let first = rows.first().ok_or(TransferError::Database)?;
    if rows.len() > 1001
        || rows
            .iter()
            .map(|row| row.source_post_id)
            .collect::<BTreeSet<_>>()
            .len()
            != rows.len()
        || first.source_fingerprint.len() != 32
        || first.source_board != source_board
        || first.source_thread != source_thread
        || first.destination_board != destination_board
        || first.source_post_id != source_thread
        || first.destination_post_id != first.destination_thread
        || rows.iter().enumerate().any(|(index, row)| {
            usize::try_from(row.ordinal).ok() != index.checked_add(1)
                || row.request_id != first.request_id
                || row.expires_at != first.expires_at
                || row.source_fingerprint != first.source_fingerprint
                || row.source_board != first.source_board
                || row.source_thread != first.source_thread
                || row.destination_board != first.destination_board
                || row.destination_thread != first.destination_thread
                || row.source_post_id <= 0
                || row.destination_post_id <= 0
        })
        || rows
            .windows(2)
            .any(|pair| pair[0].destination_post_id >= pair[1].destination_post_id)
    {
        return Err(TransferError::Database);
    }
    tx.commit().await.map_err(database_error)?;
    Ok(PreparedTransfer {
        ticket: ticket.clone(),
        rows: Arc::new(rows),
    })
}

struct SealedRows {
    old: Vec<i64>,
    new: Vec<i64>,
    comments: Vec<String>,
    presentations: Vec<Vec<u8>>,
    search: Vec<String>,
}

fn format_snapshot(rows: &[PreparedRow]) -> Result<SealedRows, TransferError> {
    let mut output = SealedRows {
        old: Vec::with_capacity(rows.len()),
        new: Vec::with_capacity(rows.len()),
        comments: Vec::with_capacity(rows.len()),
        presentations: Vec::with_capacity(rows.len()),
        search: Vec::with_capacity(rows.len()),
    };
    let mut replacements = Vec::with_capacity(rows.len());
    let mut budget = RewriteBudget::new(WORK_LIMIT);
    let mut bytes = 0_usize;
    for row in rows {
        let limits = if row.staff_authorized_limits {
            board_domain::PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS)
                .map_err(|_| TransferError::Format)?
        } else {
            board_domain::PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
        };
        let mut frozen = FrozenPresentation::from_saved(SavedCommentRef {
            comment: &row.comment,
            format: row.comment_format,
            source_board: &row.source_board,
            wordfilter_payload: row.wordfilter_payload.as_deref(),
            saved_presentation: row.saved_presentation.as_deref(),
            limits,
        })
        .map_err(presentation_error)?;
        frozen
            .rewrite_prior(&replacements, &mut budget)
            .map_err(presentation_error)?;
        let presentation = frozen.encode().map_err(presentation_error)?;
        let decoded =
            FrozenPresentation::decode(&presentation, limits).map_err(presentation_error)?;
        let comment = frozen.source_projection();
        let search = frozen.search_text();
        if decoded != frozen
            || decoded.source_projection() != comment
            || decoded.search_text() != search
        {
            return Err(TransferError::Format);
        }
        bytes = bytes
            .checked_add(comment.len())
            .and_then(|bytes| bytes.checked_add(presentation.len()))
            .and_then(|bytes| bytes.checked_add(search.len()))
            .ok_or(TransferError::Capacity)?;
        if bytes > BODY_LIMIT {
            return Err(TransferError::Capacity);
        }
        output.old.push(row.source_post_id);
        output.new.push(row.destination_post_id);
        output.comments.push(comment);
        output.presentations.push(presentation);
        output.search.push(search);
        replacements.push(
            PriorReplacement::new(row.source_post_id, row.destination_post_id)
                .map_err(|_| TransferError::Format)?,
        );
    }
    Ok(output)
}

pub async fn seal(
    auth_pool: &PgPool,
    credentials: &Credentials,
    prepared: &PreparedTransfer,
) -> Result<(), TransferError> {
    let permit = Arc::clone(FORMATTERS.get_or_init(|| Arc::new(Semaphore::new(2))))
        .try_acquire_owned()
        .map_err(|_| TransferError::Busy)?;
    let snapshot = Arc::clone(&prepared.rows);
    let output = tokio::task::spawn_blocking(move || {
        // A canceled HTTP future cannot release this permit while CPU work is
        // still running. The closure has no pool or capability and cannot seal.
        let _permit = permit;
        format_snapshot(&snapshot)
    })
    .await
    .map_err(|_| TransferError::Format)??;
    let mut tx = begin(auth_pool).await?;
    sqlx::query(
        "SELECT staff_identity.seal_text_thread_transfer($1,$2,$3,$4,1::smallint,$5,$6,$7,$8,$9)",
    )
    .bind(prepared.ticket.0.as_slice())
    .bind(credentials.session_hash.as_slice())
    .bind(credentials.csrf_hash.as_slice())
    .bind(&prepared.rows[0].source_fingerprint)
    .bind(&output.old)
    .bind(&output.new)
    .bind(&output.comments)
    .bind(&output.presentations)
    .bind(&output.search)
    .execute(&mut *tx)
    .await
    .map_err(database_error)?;
    tx.commit().await.map_err(database_error)
}

pub async fn consume(
    staff_pool: &PgPool,
    ticket: &Ticket,
) -> Result<TransferResult, TransferError> {
    let mut tx = begin(staff_pool).await?;
    let rows = sqlx::query_as("SELECT * FROM content.consume_text_thread_transfer($1)")
        .bind(ticket.0.as_slice())
        .fetch_all(&mut *tx)
        .await
        .map_err(database_error)?;
    let result = result_rows(rows)?.ok_or(TransferError::Database)?;
    tx.commit().await.map_err(database_error)?;
    Ok(result)
}

pub async fn discard(
    auth_pool: &PgPool,
    credentials: &Credentials,
    ticket: &Ticket,
) -> Result<(), TransferError> {
    let mut tx = begin(auth_pool).await?;
    sqlx::query("SELECT staff_identity.discard_text_thread_transfer($1,$2,$3)")
        .bind(ticket.0.as_slice())
        .bind(credentials.session_hash.as_slice())
        .bind(credentials.csrf_hash.as_slice())
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
    tx.commit().await.map_err(database_error)
}

pub async fn result(
    auth_pool: &PgPool,
    credentials: &Credentials,
    request_id: &str,
) -> Result<Option<TransferResult>, TransferError> {
    let rows = sqlx::query_as(
        "SELECT * FROM staff_identity.text_thread_transfer_result($1,$2,$3,$4,NULL,NULL)",
    )
    .bind(request_id)
    .bind(credentials.session_hash.as_slice())
    .bind(credentials.csrf_hash.as_slice())
    .bind(credentials.idle_seconds)
    .fetch_all(auth_pool)
    .await
    .map_err(database_error)?;
    result_rows(rows)
}

pub async fn result_for_source(
    auth_pool: &PgPool,
    credentials: &Credentials,
    source_board: &str,
    source_thread: i64,
) -> Result<Option<TransferResult>, TransferError> {
    let rows = sqlx::query_as(
        "SELECT * FROM staff_identity.text_thread_transfer_result(NULL,$1,$2,$3,$4,$5)",
    )
    .bind(credentials.session_hash.as_slice())
    .bind(credentials.csrf_hash.as_slice())
    .bind(credentials.idle_seconds)
    .bind(source_board)
    .bind(source_thread)
    .fetch_all(auth_pool)
    .await
    .map_err(database_error)?;
    result_rows(rows)
}
