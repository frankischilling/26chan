//! Capability-scoped optional event accounts. No runtime table access is needed.
pub mod admin;
pub mod scores;

use crate::{PgPool, StoreError};
use board_domain::source_events::{
    credentials::{Kind, Run, Session},
    market::{Order, Side},
};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct StoredRun {
    id: i64,
    kind: i16,
    starts_at: i64,
    ends_at: i64,
}
fn matching(run: Run, session: &Session) -> Result<(), StoreError> {
    if session.run() != run {
        return Err(protocol());
    }
    Ok(())
}
fn protocol() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol("Invalid event response.".into()))
}

pub async fn active_run(pool: &PgPool, kind: Kind) -> Result<Option<Run>, StoreError> {
    let row: Option<StoredRun> =
        sqlx::query_as("SELECT id,kind,starts_at,ends_at FROM content.event_active_run($1)")
            .bind(kind as i16)
            .fetch_optional(pool)
            .await?;
    row.map(|row| {
        if row.kind != kind as i16 {
            return Err(protocol());
        }
        Run::new(kind, row.id, row.starts_at, row.ends_at).map_err(|_| protocol())
    })
    .transpose()
}

pub async fn account(
    pool: &PgPool,
    run: Run,
    session: &Session,
) -> Result<Option<Value>, StoreError> {
    matching(run, session)?;
    sqlx::query_scalar("SELECT content.event_account($1,$2)")
        .bind(run.id())
        .bind(session.storage_hash().as_slice())
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

/// `established` must come from a verified existing anonymous session whose
/// source network-mask age is at least one hour, never a request checkbox.
pub async fn open_account(
    pool: &PgPool,
    run: Run,
    session: &Session,
    established: bool,
) -> Result<Option<Value>, StoreError> {
    matching(run, session)?;
    sqlx::query_scalar("SELECT content.event_open_account($1,$2,$3)")
        .bind(run.id())
        .bind(session.storage_hash().as_slice())
        .bind(established)
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

pub async fn market_trade(
    pool: &PgPool,
    run: Run,
    session: &Session,
    order: Order,
) -> Result<Option<Value>, StoreError> {
    matching(run, session)?;
    if run.kind() != Kind::Market {
        return Err(protocol());
    }
    let stock = board_domain::source_events::STOCKS
        .iter()
        .position(|stock| *stock == order.stock())
        .ok_or_else(protocol)? as i16;
    let side: i16 = match order.side() {
        Side::Buy => 1,
        Side::Sell => 2,
    };
    sqlx::query_scalar("SELECT content.event_market_trade($1,$2,$3,$4,$5,$6)")
        .bind(run.id())
        .bind(session.storage_hash().as_slice())
        .bind(side)
        .bind(stock)
        .bind(order.amount() as i32)
        .bind(order.price())
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

pub async fn buy_emote(
    pool: &PgPool,
    run: Run,
    session: &Session,
    id: &str,
) -> Result<Option<Value>, StoreError> {
    matching(run, session)?;
    if run.kind() != Kind::Emotes || id.len() > 64 {
        return Err(protocol());
    }
    sqlx::query_scalar("SELECT content.event_emote_acquire($1,$2,$3,0::smallint)")
        .bind(run.id())
        .bind(session.storage_hash().as_slice())
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

/// SQL generates every lottery draw while holding the account lock. Callers
/// cannot supply an emote list or random seed through this interface.
pub async fn roll_emotes(
    pool: &PgPool,
    run: Run,
    session: &Session,
    count: u8,
) -> Result<Option<Value>, StoreError> {
    matching(run, session)?;
    if run.kind() != Kind::Emotes || !matches!(count, 3 | 10) {
        return Err(protocol());
    }
    sqlx::query_scalar("SELECT content.event_emote_acquire($1,$2,NULL,$3)")
        .bind(run.id())
        .bind(session.storage_hash().as_slice())
        .bind(i16::from(count))
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

/// Operator-provided quotes for the reconstructed local market page. No future
/// or closed run prices are exposed, and absent prices remain absent.
pub async fn market_quotes(pool: &PgPool, run: Run) -> Result<Option<Value>, StoreError> {
    if run.kind() != Kind::Market {
        return Err(protocol());
    }
    sqlx::query_scalar("SELECT content.event_market_quotes($1)")
        .bind(run.id())
        .fetch_one(pool)
        .await
        .map_err(StoreError::from)
}

pub async fn ready(pool: &PgPool) -> Result<(), StoreError> {
    if sqlx::query_scalar::<_, bool>("SELECT content.events_ready()")
        .fetch_one(pool)
        .await?
    {
        Ok(())
    } else {
        Err(StoreError::Invalid("Event authority is unavailable."))
    }
}
