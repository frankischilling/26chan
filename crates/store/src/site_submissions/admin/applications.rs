//! Campaign configuration is an explicit operator action, never a public form.
use super::{Error, transaction};
use board_domain::site_submissions::applications::Profile;
use serde::Deserialize;
use sqlx::PgConnection;
use std::io::Read;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Campaign {
    version: u8,
    starts_at: i64,
    ends_at: i64,
    boards: Vec<String>,
    questions: [String; 4],
    field_limit: u16,
    answer_limit: u16,
    capacity: i32,
    cooldown_seconds: i32,
}
impl Campaign {
    fn validate(&self) -> Result<(), Error> {
        if self.version != 1
            || self.starts_at < 0
            || self.ends_at <= self.starts_at
            || self.ends_at > 253402300799
            || !(1..=100000).contains(&self.capacity)
            || !(0..=86400).contains(&self.cooldown_seconds)
        {
            return Err(Error::ApplicationInput);
        }
        Profile {
            boards: self.boards.clone(),
            questions: self.questions.clone(),
            field_limit: usize::from(self.field_limit),
            answer_limit: usize::from(self.answer_limit),
        }
        .validate()
        .map_err(|_| Error::ApplicationInput)
    }
}
pub fn read(reader: impl Read) -> Result<Campaign, Error> {
    let mut bytes = Vec::new();
    reader
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::ApplicationInput)?;
    if bytes.len() > 32768 {
        return Err(Error::ApplicationInput);
    }
    let campaign: Campaign = serde_json::from_slice(&bytes).map_err(|_| Error::ApplicationInput)?;
    campaign.validate()?;
    Ok(campaign)
}
pub async fn open(connection: &mut PgConnection, campaign: &Campaign) -> Result<i64, Error> {
    campaign.validate()?;
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.application_campaigns IN EXCLUSIVE MODE",
    )
    .await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM submission_private.application_campaigns WHERE enabled)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if active {
        tx.rollback().await?;
        return Err(Error::ApplicationActive);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM submission_private.application_campaigns")
            .fetch_one(&mut *tx)
            .await?;
    if count >= 10000 {
        tx.rollback().await?;
        return Err(Error::ApplicationFull);
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(id),0)+1 FROM submission_private.application_campaigns",
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO submission_private.application_campaigns(id,enabled,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity,cooldown_seconds) VALUES($1,true,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(campaign.starts_at).bind(campaign.ends_at).bind(&campaign.boards).bind(&campaign.questions[..])
        .bind(i32::from(campaign.field_limit)).bind(i32::from(campaign.answer_limit)).bind(campaign.capacity).bind(campaign.cooldown_seconds)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}
pub async fn close(connection: &mut PgConnection, id: i64) -> Result<bool, Error> {
    if id <= 0 {
        return Err(Error::ApplicationInput);
    }
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.application_campaigns IN EXCLUSIVE MODE",
    )
    .await?;
    let changed = sqlx::query(
        "UPDATE submission_private.application_campaigns SET enabled=false WHERE id=$1 AND enabled",
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
    const JSON: &str = r#"{"version":1,"starts_at":1,"ends_at":2000000000,"boards":["g"],"questions":["One","Two","Three","Four"],"field_limit":150,"answer_limit":2000,"capacity":100,"cooldown_seconds":60}"#;
    #[test]
    fn campaign_files_are_bounded_exact_and_domain_checked() {
        assert!(read(JSON.as_bytes()).is_ok());
        for bad in [
            JSON.replace("\"version\":1", "\"version\":2"),
            JSON.replace("\"version\":1", "\"version\":1,\"version\":1"),
            JSON.replace("\"starts_at\":1", "\"starts_at\":-1"),
            JSON.replace("2000000000", "1"),
            JSON.replace("2000000000", "253402300800"),
            JSON.replace("[\"g\"]", "[\"g\",\"g\"]"),
            JSON.replace("\"Four\"", "\"\""),
            JSON.replace("\"capacity\":100", "\"capacity\":0"),
            JSON.replace("\"cooldown_seconds\":60", "\"cooldown_seconds\":86401"),
            JSON.replace("\"version\":1", "\"version\":1,\"unknown\":0"),
            " ".repeat(32769),
        ] {
            assert!(read(bad.as_bytes()).is_err(), "{bad}");
        }
    }
}
