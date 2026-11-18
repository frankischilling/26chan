//! Approved public listings and one Pass vote per campaign/board.
use super::{decode, feedback::Proof, protocol};
use crate::{PgPool, StoreError};
use board_domain::site_submissions::contests::{Campaign, Kind};
use serde::Deserialize;
pub async fn vote(
    pool: &PgPool,
    campaign: i64,
    entry: i64,
    proof: &Proof<'_>,
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT submission_api.contest_vote($1,$2,$3,$4,$5,$6)")
        .bind(campaign)
        .bind(entry)
        .bind(proof.receipt.as_slice())
        .bind(proof.peer.as_slice())
        .bind(proof.session.as_slice())
        .bind(proof.context.as_slice())
        .fetch_one(pool)
        .await
        .map_err(refusal)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: i64,
    pub board: String,
    pub media_id: String,
    pub score: Option<i32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub campaign: i64,
    pub board: String,
    pub voting: bool,
    pub visible: bool,
    pub entries: Vec<Entry>,
    pub offset: i32,
    pub next: Option<i32>,
}
pub async fn listing(
    pool: &PgPool,
    campaign: i64,
    board: &str,
    offset: i32,
) -> Result<Option<Listing>, StoreError> {
    let value: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.contest_listing($1,$2,$3)")
            .bind(campaign)
            .bind(board)
            .bind(offset)
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    value
        .map(|value| {
            let page: Listing = decode(value)?;
            if page.campaign != campaign
                || page.board != board
                || page.offset != offset
                || !(0..=100000).contains(&offset)
                || page.entries.len() > 25
                || page
                    .next
                    .is_some_and(|n| n != offset + 25 || page.entries.len() != 25)
                || (!page.visible && (!page.entries.is_empty() || page.next.is_some()))
            {
                return Err(protocol());
            }
            let mut previous = i64::MAX;
            for entry in &page.entries {
                if entry.id <= 0
                    || entry.id >= previous
                    || entry.board != board
                    || entry.media_id.len() != 32
                    || !entry
                        .media_id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    || (page.voting && entry.score.is_some())
                    || (!page.voting && entry.score.is_none())
                    || entry.score.is_some_and(|n| !(0..=1000000).contains(&n))
                {
                    return Err(protocol());
                }
                previous = entry.id;
            }
            Ok(page)
        })
        .transpose()
}
pub async fn campaign(pool: &PgPool, id: i64) -> Result<Option<super::ContestProfile>, StoreError> {
    let value: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.contest_campaign($1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    value
        .map(|value| {
            let raw: super::RawContest = decode(value)?;
            let kind = match raw.kind {
                1 => Kind::BoardBanner,
                2 => Kind::Vtuber,
                _ => return Err(protocol()),
            };
            if raw.id != id || !(0..=253402300799).contains(&raw.current_time) {
                return Err(protocol());
            }
            let campaign = Campaign {
                id,
                kind,
                boards: raw.boards,
                submission_starts: raw.submission_starts,
                submission_ends: raw.submission_ends,
                voting_starts: raw.voting_starts,
                voting_ends: raw.voting_ends,
                capacity: raw.capacity,
                enabled: true,
            };
            campaign.validate().map_err(|_| protocol())?;
            Ok(super::ContestProfile {
                campaign,
                current_time: raw.current_time,
            })
        })
        .transpose()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub id: i64,
    pub boards: Vec<String>,
    pub submission_starts: i64,
    pub submission_ends: i64,
    pub voting_starts: i64,
    pub voting_ends: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    kind: i16,
    pub campaigns: Vec<Schedule>,
    pub next: Option<i64>,
    pub current_time: i64,
}
pub async fn directory(pool: &PgPool, kind: Kind, before: i64) -> Result<Directory, StoreError> {
    let number = match kind {
        Kind::BoardBanner => 1_i16,
        Kind::Vtuber => 2,
    };
    let raw: serde_json::Value =
        sqlx::query_scalar("SELECT submission_api.contest_directory($1,$2)")
            .bind(number)
            .bind(before)
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    let page: Directory = decode(raw)?;
    if page.kind != number
        || page.campaigns.len() > 16
        || !(0..=253402300799).contains(&page.current_time)
        || page.next.is_some_and(|n| {
            page.campaigns.len() != 16 || page.campaigns.last().is_none_or(|last| last.id != n)
        })
    {
        return Err(protocol());
    }
    let mut previous = if before == 0 { i64::MAX } else { before };
    for row in &page.campaigns {
        if row.id <= 0 || row.id >= previous {
            return Err(protocol());
        }
        previous = row.id;
        Campaign {
            id: row.id,
            kind,
            boards: row.boards.clone(),
            submission_starts: row.submission_starts,
            submission_ends: row.submission_ends,
            voting_starts: row.voting_starts,
            voting_ends: row.voting_ends,
            capacity: 1,
            enabled: true,
        }
        .validate()
        .map_err(|_| protocol())?;
    }
    Ok(page)
}
fn refusal(error: sqlx::Error) -> StoreError {
    if let Some(database) = error.as_database_error() {
        if database.code().as_deref() == Some("P0166") {
            return StoreError::Invalid("Valid Pass verification is required.");
        }
        if database.code().as_deref() == Some("P0002") {
            return StoreError::Invalid("Contest voting is closed.");
        }
        if database.code().as_deref() == Some("P225V") {
            for message in [
                "Invalid contest vote.",
                "Contest voting is closed.",
                "Contest entry is unavailable.",
                "Contest vote capacity has been reached.",
                "Invalid contest listing.",
                "Invalid contest campaign.",
                "Invalid contest directory.",
            ] {
                if database.message() == message {
                    return StoreError::Invalid(message);
                }
            }
        }
    }
    StoreError::Database(error)
}
