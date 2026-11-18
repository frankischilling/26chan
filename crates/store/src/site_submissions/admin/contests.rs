//! Explicit contest schedules may overlap for different boards.
use super::{Error, transaction};
use board_domain::site_submissions::contests::{Campaign as Domain, Kind};
use serde::Deserialize;
use sqlx::PgConnection;
use std::io::Read;
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContestKind {
    BoardBanner,
    Vtuber,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    version: u8,
    kind: ContestKind,
    boards: Vec<String>,
    submission_starts: i64,
    submission_ends: i64,
    voting_starts: i64,
    voting_ends: i64,
    capacity: u32,
    vote_capacity: i32,
}
impl Campaign {
    fn kind(&self) -> Kind {
        match self.kind {
            ContestKind::BoardBanner => Kind::BoardBanner,
            ContestKind::Vtuber => Kind::Vtuber,
        }
    }
    fn number(&self) -> i16 {
        match self.kind {
            ContestKind::BoardBanner => 1,
            ContestKind::Vtuber => 2,
        }
    }
    fn validate(&self) -> Result<(), Error> {
        if self.version != 1 || !(1..=1000000).contains(&self.vote_capacity) {
            return Err(Error::ContestInput);
        }
        Domain {
            id: 1,
            kind: self.kind(),
            boards: self.boards.clone(),
            submission_starts: self.submission_starts,
            submission_ends: self.submission_ends,
            voting_starts: self.voting_starts,
            voting_ends: self.voting_ends,
            capacity: self.capacity,
            enabled: true,
        }
        .validate()
        .map_err(|_| Error::ContestInput)
    }
}
pub fn read(reader: impl Read) -> Result<Campaign, Error> {
    let mut bytes = Vec::new();
    reader
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::ContestInput)?;
    if bytes.len() > 32768 {
        return Err(Error::ContestInput);
    }
    let campaign: Campaign = serde_json::from_slice(&bytes).map_err(|_| Error::ContestInput)?;
    campaign.validate()?;
    Ok(campaign)
}
pub async fn open(connection: &mut PgConnection, campaign: &Campaign) -> Result<i64, Error> {
    campaign.validate()?;
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.contest_campaigns IN EXCLUSIVE MODE",
    )
    .await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM submission_private.contest_campaigns")
            .fetch_one(&mut *tx)
            .await?;
    if count >= 10000 {
        tx.rollback().await?;
        return Err(Error::ContestFull);
    }
    let overlaps:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM submission_private.contest_campaigns WHERE enabled AND kind=$1 AND boards && $2 AND int8range(submission_starts,voting_ends,'[)') && int8range($3,$4,'[)'))")
        .bind(campaign.number()).bind(&campaign.boards).bind(campaign.submission_starts).bind(campaign.voting_ends).fetch_one(&mut *tx).await?;
    if overlaps {
        tx.rollback().await?;
        return Err(Error::ContestOverlap);
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(id),0)+1 FROM submission_private.contest_campaigns",
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO submission_private.contest_campaigns(id,kind,enabled,boards,submission_starts,submission_ends,voting_starts,voting_ends,capacity,vote_capacity) VALUES($1,$2,true,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(campaign.number()).bind(&campaign.boards).bind(campaign.submission_starts).bind(campaign.submission_ends).bind(campaign.voting_starts).bind(campaign.voting_ends).bind(campaign.capacity as i32).bind(campaign.vote_capacity).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn close(connection: &mut PgConnection, id: i64) -> Result<bool, Error> {
    if id <= 0 {
        return Err(Error::ContestInput);
    }
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.contest_campaigns IN EXCLUSIVE MODE",
    )
    .await?;
    let changed = sqlx::query(
        "UPDATE submission_private.contest_campaigns SET enabled=false WHERE id=$1 AND enabled",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    tx.commit().await?;
    Ok(changed)
}
#[cfg(test)]
mod tests {
    use super::*;
    const PROFILE: &str = r#"{"version":1,"kind":"board_banner","boards":["g"],"submission_starts":100,"submission_ends":200,"voting_starts":300,"voting_ends":400,"capacity":100,"vote_capacity":1000}"#;
    #[test]
    fn contest_configuration_is_bounded_exact_and_phase_checked() {
        assert!(read(PROFILE.as_bytes()).is_ok());
        for bad in [
            PROFILE.replace("\"version\":1", "\"version\":1,\"version\":1"),
            PROFILE.replace("\"version\":1", "\"version\":2"),
            PROFILE.replace("\"version\":1", "\"version\":1,\"extra\":0"),
            PROFILE.replace("board_banner", "other"),
            PROFILE.replace("[\"g\"]", "[\"g\",\"g\"]"),
            PROFILE.replace("[\"g\"]", "[\"all\"]"),
            PROFILE.replace("\"voting_starts\":300", "\"voting_starts\":199"),
            PROFILE.replace("\"capacity\":100", "\"capacity\":100001"),
            PROFILE.replace("\"vote_capacity\":1000", "\"vote_capacity\":0"),
            " ".repeat(32769),
        ] {
            assert!(read(bad.as_bytes()).is_err());
        }
        assert!(read(PROFILE.replace("board_banner", "vtuber").as_bytes()).is_err());
        assert!(
            read(
                PROFILE
                    .replace("board_banner", "vtuber")
                    .replace("[\"g\"]", "[\"all\"]")
                    .as_bytes()
            )
            .is_ok()
        );
    }
}
