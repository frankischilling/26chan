//! Explicit migrator-only configuration. Closing retains submissions.
pub mod applications;
pub mod contests;
use board_domain::site_submissions::feedback::{Category, Profile as FormProfile};
use serde::Deserialize;
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use std::io::Read;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "Expected bounded version-1 feedback JSON with valid categories, limits, capacity and cooldown."
    )]
    Input,
    #[error(
        "Expected bounded version-1 application campaign JSON with valid dates, boards, questions and limits."
    )]
    ApplicationInput,
    #[error("An application campaign is already enabled. Close it first.")]
    ApplicationActive,
    #[error("The retained application campaign budget is full.")]
    ApplicationFull,
    #[error("Expected bounded version-1 contest JSON with valid kind, boards, dates and limits.")]
    ContestInput,
    #[error("An enabled contest for this kind and board overlaps the requested schedule.")]
    ContestOverlap,
    #[error("The retained contest campaign budget is full.")]
    ContestFull,
    #[error("Submission configuration requires the actual board_migrator login.")]
    Role,
    #[error("Submission authority has drifted; configuration was not changed.")]
    Authority,
    #[error("Feedback is already enabled. Close the current profile first.")]
    Active,
    #[error("The retained feedback-profile budget is full.")]
    Full,
    #[error(
        "Submission database operation failed; credentials and server details are not displayed."
    )]
    Database,
}
impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputCategory {
    id: u16,
    label: String,
    short_label: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackProfile {
    version: u8,
    subject_limit: u16,
    message_limit: u16,
    capacity: i32,
    cooldown_seconds: i32,
    categories: Vec<InputCategory>,
}
impl FeedbackProfile {
    fn validate(&self) -> Result<(), Error> {
        if self.version != 1
            || !(1..=100000).contains(&self.capacity)
            || !(0..=86400).contains(&self.cooldown_seconds)
        {
            return Err(Error::Input);
        }
        FormProfile {
            subject_limit: usize::from(self.subject_limit),
            message_limit: usize::from(self.message_limit),
            categories: self
                .categories
                .iter()
                .map(|c| Category {
                    id: c.id,
                    label: c.label.clone(),
                    short_label: c.short_label.clone(),
                })
                .collect(),
        }
        .validate()
        .map_err(|_| Error::Input)
    }
}
pub fn read_feedback(reader: impl Read) -> Result<FeedbackProfile, Error> {
    let mut bytes = Vec::new();
    reader
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    if bytes.len() > 32768 {
        return Err(Error::Input);
    }
    let profile: FeedbackProfile = serde_json::from_slice(&bytes).map_err(|_| Error::Input)?;
    profile.validate()?;
    Ok(profile)
}
async fn transaction<'a>(
    connection: &'a mut PgConnection,
    lock: &'static str,
) -> Result<Transaction<'a, Postgres>, Error> {
    let mut tx = connection.begin().await?;
    let allowed: bool = sqlx::query_scalar(
        "SELECT current_user='board_migrator' AND session_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await?;
    if !allowed {
        tx.rollback().await?;
        return Err(Error::Role);
    }
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *tx)
        .await?;
    let ready: bool = sqlx::query_scalar("SELECT submission_api.ready()")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("RESET ROLE").execute(&mut *tx).await?;
    if !ready {
        tx.rollback().await?;
        return Err(Error::Authority);
    }
    // Conflict with row-lock acquisition too, so a writer cannot hold the row
    // while waiting to obtain the table lock needed for its counter update.
    sqlx::query(lock).execute(&mut *tx).await?;
    Ok(tx)
}
pub async fn start_feedback(
    connection: &mut PgConnection,
    profile: &FeedbackProfile,
) -> Result<i64, Error> {
    profile.validate()?;
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.feedback_profiles IN EXCLUSIVE MODE",
    )
    .await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM submission_private.feedback_profiles WHERE enabled)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if active {
        tx.rollback().await?;
        return Err(Error::Active);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM submission_private.feedback_profiles")
            .fetch_one(&mut *tx)
            .await?;
    if count >= 10000 {
        tx.rollback().await?;
        return Err(Error::Full);
    }
    let id: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(id),0)+1 FROM submission_private.feedback_profiles",
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO submission_private.feedback_profiles(id,enabled,subject_limit,message_limit,capacity,cooldown_seconds) VALUES($1,true,$2,$3,$4,$5)")
        .bind(id).bind(i32::from(profile.subject_limit)).bind(i32::from(profile.message_limit)).bind(profile.capacity).bind(profile.cooldown_seconds).execute(&mut *tx).await?;
    for (ordinal, category) in profile.categories.iter().enumerate() {
        sqlx::query("INSERT INTO submission_private.feedback_categories(profile_id,id,ordinal,label,short_label) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind(i32::from(category.id)).bind(ordinal as i16).bind(&category.label).bind(&category.short_label).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(id)
}
pub async fn close_feedback(connection: &mut PgConnection, id: i64) -> Result<bool, Error> {
    if id <= 0 {
        return Err(Error::Input);
    }
    let mut tx = transaction(
        connection,
        "LOCK TABLE submission_private.feedback_profiles IN EXCLUSIVE MODE",
    )
    .await?;
    let changed = sqlx::query(
        "UPDATE submission_private.feedback_profiles SET enabled=false WHERE id=$1 AND enabled",
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
    const PROFILE: &str = r#"{"version":1,"subject_limit":150,"message_limit":2000,"capacity":100,"cooldown_seconds":60,"categories":[{"id":1,"label":"Owned","short_label":"Owned"}]}"#;
    #[test]
    fn offline_feedback_profiles_are_exact_bounded_and_domain_validated() {
        assert!(read_feedback(PROFILE.as_bytes()).is_ok());
        for input in [
            PROFILE.replace("\"version\":1", "\"version\":2"),
            PROFILE.replace("\"capacity\":100", "\"capacity\":0"),
            PROFILE.replace("\"capacity\":100", "\"capacity\":100001"),
            PROFILE.replace("\"cooldown_seconds\":60", "\"cooldown_seconds\":86401"),
            PROFILE.replace("\"subject_limit\":150", "\"subject_limit\":513"),
            PROFILE.replace("\"version\":1", "\"version\":1,\"version\":1"),
            PROFILE.replace("\"version\":1", "\"version\":1,\"extra\":true"),
            PROFILE.replace("\"label\":\"Owned\"", "\"label\":\"\""),
            " ".repeat(32769),
        ] {
            assert!(read_feedback(input.as_bytes()).is_err(), "{input}");
        }
        let duplicate = PROFILE.replace(
            "}]}",
            "},{\"id\":1,\"label\":\"Again\",\"short_label\":\"Again\"}]}",
        );
        assert!(read_feedback(duplicate.as_bytes()).is_err());
    }
}
