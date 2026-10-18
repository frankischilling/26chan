//! Private per-attempt diagnostics, separate from authoritative cooldown state.
use crate::StoreError;
use board_domain::content_admission::{classifiers::posting, threat::Score};
use sqlx::{Postgres, Transaction};

#[derive(Clone, Copy)]
pub enum Event {
    Prefix(posting::Event),
    History(posting::history::Event),
}
impl Event {
    fn kind(self) -> &'static str {
        match self {
            Self::Prefix(event) => event.source_name(),
            Self::History(posting::history::Event::LogEdgeCooldown) => "block_bm_bot_pwd",
            Self::History(posting::history::Event::RecordEdgeBlock) => "block_bm_bot",
            Self::History(posting::history::Event::RecordCountryChange) => "country_changed",
            Self::History(posting::history::Event::LogCountryBlock) => "block_country_changed",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Written {
    Recorded,
    CapacityDropped,
}

/// Persist an ordered plan without changing its admission outcome. At capacity
/// the entire diagnostic batch is dropped; a refusal must remain a refusal and
/// a nonblocking log must not become a new posting restriction. No credential,
/// header, peer address, comment or filename can enter through this interface.
pub async fn record(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    thread: i64,
    events: &[Event],
    country: Option<&str>,
    score: Option<Score>,
) -> Result<Written, StoreError> {
    if events.is_empty() || events.len() > 3 {
        return Err(StoreError::Invalid("Invalid classifier diagnostic plan."));
    }
    let kinds: Vec<_> = events.iter().map(|event| event.kind()).collect();
    let recorded: bool =
        sqlx::query_scalar("SELECT content.record_posting_classifier_events($1,$2,$3,$4,$5)")
            .bind(board)
            .bind(thread)
            .bind(kinds)
            .bind(country)
            .bind(score.map(|score| score.value()))
            .fetch_one(&mut **tx)
            .await?;
    Ok(if recorded {
        Written::Recorded
    } else {
        Written::CapacityDropped
    })
}
