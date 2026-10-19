//! Server-derived identity binding for transactional posting restrictions.
use crate::StoreError;
use board_domain::poster_id::PublicPostingRateIdentity;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};

/// Contains only the matching subject's public status. Private reasons,
/// transport/session equality and staff accounts never leave the definer.
#[derive(sqlx::FromRow)]
pub struct Status {
    pub id: i64,
    pub board: String,
    pub global: bool,
    pub warning: bool,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub public_reason: String,
}

pub async fn status(
    pool: &PgPool,
    actor: &PublicPostingRateIdentity,
    token: Option<&[u8; 32]>,
    board: &str,
    request_at: i64,
) -> Result<Vec<Status>, StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    let rows = sqlx::query_as("SELECT * FROM content.restriction_status($1,$2,$3,$4)")
        .bind(actor.as_bytes().as_slice())
        .bind(token.map(|token| token.as_slice()))
        .bind(board)
        .bind(request_at)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows)
}

pub async fn acknowledge(
    pool: &PgPool,
    actor: &PublicPostingRateIdentity,
    token: Option<&[u8; 32]>,
    board: &str,
    request_at: i64,
    id: i64,
) -> Result<bool, StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    let changed = sqlx::query_scalar("SELECT content.acknowledge_warning($1,$2,$3,$4,$5)")
        .bind(actor.as_bytes().as_slice())
        .bind(token.map(|token| token.as_slice()))
        .bind(board)
        .bind(request_at)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(changed)
}

pub const BANNED: &str = "Error: You are banned.";
pub const WARNED: &str = "Error: You were warned. You must first view this warning to post again.";
pub const REPORT_BANNED: &str = "You can't report posts because you are banned.";

/// Real staff credentials and source thread-batch fields. Never deserialize
/// authority from a request; the definer revalidates it at transaction commit.
pub struct ThreadBan<'a> {
    pub session: &'a [u8],
    pub csrf: &'a [u8],
    pub idle_seconds: i32,
    pub board: &'a str,
    pub thread: i64,
    pub op_reason: &'a str,
    pub reply_reason: &'a str,
    pub op_days: i32,
    pub reply_days: i32,
}

/// Bind both independently derived peer identities before any report locks.
/// Ban evaluation happens after target and quota checks inside SQL admission.
/// The shared gate prevents issuance/retirement until the report commits.
pub(crate) async fn prepare_report(
    tx: &mut Transaction<'_, Postgres>,
    actor: &board_domain::poster_id::PublicReportRateIdentity,
    token: Option<&[u8; 32]>,
    board: &str,
    post: i64,
    request_at: i64,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.bind_report_restrictions($1,$2,$3,$4,$5,$6)")
        .bind(actor.as_bytes().as_slice())
        .bind(actor.restriction_identity().as_bytes().as_slice())
        .bind(token.map(|token| token.as_slice()))
        .bind(board)
        .bind(post)
        .bind(request_at)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Called after the actor gate and before content/session/media locks. The
/// private shared restriction gate stays held until commit, serializing the
/// decision with issuance, retirement and warning acknowledgement.
pub(crate) async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    token: Option<&[u8; 32]>,
    board: &str,
    request_at: i64,
    member_target: Option<i64>,
) -> Result<Option<&'static str>, StoreError> {
    // Always replace the local context, including the absent-token case: a
    // previously configured pooled connection must not supply cookie authority.
    let encoded = token
        .map(|token| {
            token
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .unwrap_or_default();
    sqlx::query("SELECT set_config('board.restriction_token',$1,true)")
        .bind(encoded)
        .execute(&mut **tx)
        .await?;
    let statement = if member_target.is_some() {
        "SELECT content.evaluate_member_posting_restrictions($1,$2,$3,$4,$5)"
    } else {
        "SELECT content.evaluate_posting_restrictions($1,$2,$3,$4)"
    };
    let mut query = sqlx::query_scalar(statement)
        .bind(actor.as_bytes().as_slice())
        .bind(token.map(|token| token.as_slice()))
        .bind(board)
        .bind(request_at);
    if let Some(target) = member_target {
        query = query.bind(target);
    }
    let decision: String = query.fetch_one(&mut **tx).await?;
    match decision.as_str() {
        "allowed" => Ok(None),
        "banned" => Ok(Some(BANNED)),
        "warned" => Ok(Some(WARNED)),
        _ => Err(StoreError::Database(sqlx::Error::Protocol(
            "Restriction decision is unavailable.".into(),
        ))),
    }
}

/// The insert trigger remains authoritative even for internal/direct writers.
/// Only fixed public messages are exposed; private database text is discarded.
pub(crate) fn insertion_error(error: sqlx::Error) -> StoreError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("P134B") => StoreError::ContentRejected(BANNED.into()),
        Some("P134W") => StoreError::ContentRejected(WARNED.into()),
        _ => StoreError::Database(error),
    }
}
