//! Optional score mutations consume distinct, target-bound verification proofs.
use crate::{PgPool, StoreError};

#[derive(Clone, Copy, Debug, sqlx::FromRow)]
pub struct Run {
    pub id: i64,
    pub starts_at: i64,
    pub ends_at: i64,
}
pub async fn active_run(pool: &PgPool) -> Result<Option<Run>, StoreError> {
    let run: Option<Run> =
        sqlx::query_as("SELECT id,starts_at,ends_at FROM content.event_active_run(3::smallint)")
            .fetch_optional(pool)
            .await?;
    if run.is_some_and(|r| {
        r.id <= 0 || r.starts_at < 0 || r.ends_at <= r.starts_at || r.ends_at > 253402300799
    }) {
        return Err(super::protocol());
    }
    Ok(run)
}
/// Every identity and address come from the trusted transport, never a form.
/// SQL evaluates global ranges under its policy lock, then validates and consumes
/// the one-use CAPTCHA or Pass proof before committing the complete ledger.
pub struct Like<'a> {
    pub run: i64,
    pub board: &'a str,
    pub post: i64,
    pub actor: &'a [u8; 32],
    pub session: &'a [u8; 32],
    pub receipt: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub context: &'a [u8; 32],
    pub address: std::net::IpAddr,
}
pub async fn like(pool: &PgPool, input: &Like<'_>) -> Result<i32, StoreError> {
    let count: i32 = sqlx::query_scalar("SELECT content.event_like($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(input.run)
        .bind(input.board)
        .bind(input.post)
        .bind(input.actor.as_slice())
        .bind(input.session.as_slice())
        .bind(input.receipt.as_slice())
        .bind(input.peer.as_slice())
        .bind(input.context.as_slice())
        .bind(match input.address.to_canonical() {
            std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(i64::from(u32::from(ip))),
            _ => None,
        })
        .fetch_one(pool)
        .await
        .map_err(refusal)?;
    if !(1..1048576).contains(&count) {
        return Err(super::protocol());
    }
    Ok(count)
}

fn refusal(error: sqlx::Error) -> StoreError {
    if error.as_database_error().is_some_and(|e| {
        e.code().as_deref() == Some("P0166") && e.message() == "Like verification is required."
    }) {
        return StoreError::Invalid("Like verification is required.");
    }

    if let Some(database) = error.as_database_error()
        && database.code().as_deref() == Some("P0224")
    {
        for message in [
            "Bad Request.",
            "Score event is closed.",
            "You can't like this post.",
            "You can't like your own posts.",
            "You already like this post.",
            "You have to wait a while before liking this post.",
            "This post doesn't exist anymore.",
            "You can't like this post anymore.",
            "Event account capacity reached.",
            "Event vote capacity reached.",
        ] {
            if database.message() == message {
                return StoreError::Invalid(message);
            }
        }
    }
    StoreError::Database(error)
}

#[derive(sqlx::FromRow)]
pub struct Target {
    pub thread_id: i64,
    pub worksafe: bool,
    pub likes: i32,
}
pub async fn target(
    pool: &PgPool,
    run: i64,
    board: &str,
    post: i64,
) -> Result<Option<Target>, StoreError> {
    let target: Option<Target> =
        sqlx::query_as("SELECT * FROM content.event_like_target($1,$2,$3)")
            .bind(run)
            .bind(board)
            .bind(post)
            .fetch_optional(pool)
            .await?;
    if target
        .as_ref()
        .is_some_and(|t| t.thread_id <= 0 || !(0..1048576).contains(&t.likes))
    {
        return Err(super::protocol());
    }
    Ok(target)
}

/// Read-only advice. A later mutation rechecks the same private history.
pub async fn known_address(
    pool: &PgPool,
    run: i64,
    actor: &[u8; 32],
    session: &[u8; 32],
) -> Result<bool, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT content.event_like_exemption($1,$2,$3)")
            .bind(run)
            .bind(actor.as_slice())
            .bind(session.as_slice())
            .fetch_one(pool)
            .await?,
    )
}
/// Only use when source authentication cookies and an active native Pass are
/// absent. SQL still requires prior anonymous votes and a live unverified session.
pub struct KnownLike<'a> {
    pub run: i64,
    pub board: &'a str,
    pub post: i64,
    pub actor: &'a [u8; 32],
    pub session: &'a [u8; 32],
    pub address: std::net::IpAddr,
}
pub async fn like_known(pool: &PgPool, input: &KnownLike<'_>) -> Result<i32, StoreError> {
    let count: i32 = sqlx::query_scalar("SELECT content.event_like_known($1,$2,$3,$4,$5,$6)")
        .bind(input.run)
        .bind(input.board)
        .bind(input.post)
        .bind(input.actor.as_slice())
        .bind(input.session.as_slice())
        .bind(match input.address.to_canonical() {
            std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(i64::from(u32::from(ip))),
            _ => None,
        })
        .fetch_one(pool)
        .await
        .map_err(refusal)?;
    if !(1..1048576).contains(&count) {
        return Err(super::protocol());
    }
    Ok(count)
}
