//! Narrow configuration readers for native site submissions. Personal records
//! never pass through these readers. Mutations have separate proof boundaries.
pub mod admin;
pub mod applications;
pub mod feedback;
use crate::{PgPool, StoreError};
use board_domain::site_submissions::{
    applications as application_domain, contests, feedback as feedback_domain,
};
use serde::Deserialize;

pub struct FeedbackProfile {
    pub id: i64,
    pub form: feedback_domain::Profile,
}
pub struct ApplicationProfile {
    pub id: i64,
    pub form: application_domain::Profile,
}
pub struct ContestProfile {
    pub campaign: contests::Campaign,
    pub current_time: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCategory {
    id: u16,
    label: String,
    short_label: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFeedback {
    id: i64,
    subject_limit: usize,
    message_limit: usize,
    categories: Vec<RawCategory>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplication {
    id: i64,
    boards: Vec<String>,
    questions: [String; 4],
    field_limit: usize,
    answer_limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContest {
    id: i64,
    kind: i16,
    boards: Vec<String>,
    submission_starts: i64,
    submission_ends: i64,
    voting_starts: i64,
    voting_ends: i64,
    capacity: u32,
    current_time: i64,
}
fn protocol() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol(
        "Invalid site submission profile.".into(),
    ))
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, StoreError> {
    serde_json::from_value(value).map_err(|_| protocol())
}
pub async fn ready(pool: &PgPool) -> Result<bool, StoreError> {
    Ok(sqlx::query_scalar("SELECT submission_api.ready()")
        .fetch_one(pool)
        .await?)
}
pub async fn feedback_profile(pool: &PgPool) -> Result<Option<FeedbackProfile>, StoreError> {
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.feedback_profile()")
            .fetch_one(pool)
            .await?;
    raw.map(|value| {
        let input: RawFeedback = decode(value)?;
        if input.id <= 0 {
            return Err(protocol());
        }
        let form = feedback_domain::Profile {
            categories: input
                .categories
                .into_iter()
                .map(|c| feedback_domain::Category {
                    id: c.id,
                    label: c.label,
                    short_label: c.short_label,
                })
                .collect(),
            subject_limit: input.subject_limit,
            message_limit: input.message_limit,
        };
        form.validate().map_err(|_| protocol())?;
        Ok(FeedbackProfile { id: input.id, form })
    })
    .transpose()
}
pub async fn application_profile(pool: &PgPool) -> Result<Option<ApplicationProfile>, StoreError> {
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.application_profile()")
            .fetch_one(pool)
            .await?;
    raw.map(|value| {
        let input: RawApplication = decode(value)?;
        if input.id <= 0 {
            return Err(protocol());
        }
        let form = application_domain::Profile {
            boards: input.boards,
            questions: input.questions,
            field_limit: input.field_limit,
            answer_limit: input.answer_limit,
        };
        form.validate().map_err(|_| protocol())?;
        Ok(ApplicationProfile { id: input.id, form })
    })
    .transpose()
}
pub async fn contest_profile(
    pool: &PgPool,
    kind: contests::Kind,
) -> Result<Option<ContestProfile>, StoreError> {
    let number = match kind {
        contests::Kind::BoardBanner => 1_i16,
        contests::Kind::Vtuber => 2_i16,
    };
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.contest_profile($1)")
            .bind(number)
            .fetch_one(pool)
            .await?;
    raw.map(|value| {
        let input: RawContest = decode(value)?;
        if input.kind != number || !(0..=253_402_300_799).contains(&input.current_time) {
            return Err(protocol());
        }
        let campaign = contests::Campaign {
            id: input.id,
            kind,
            boards: input.boards,
            submission_starts: input.submission_starts,
            submission_ends: input.submission_ends,
            voting_starts: input.voting_starts,
            voting_ends: input.voting_ends,
            capacity: input.capacity,
            enabled: true,
        };
        campaign.validate().map_err(|_| protocol())?;
        Ok(ContestProfile {
            campaign,
            current_time: input.current_time,
        })
    })
    .transpose()
}
