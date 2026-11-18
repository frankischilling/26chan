//! Applicant-owned create/authenticate/edit operations. Matching the contact
//! email is not proof of mailbox ownership. No private values implement Debug.
use super::{decode, feedback::Proof, protocol};
use crate::{PgPool, StoreError};
use board_domain::site_submissions::{
    ContactEmail,
    applications::{self, Submission},
    ownership::{Capability, Kind},
};
use serde::Deserialize;

fn owner(capability: &Capability) -> Result<&[u8], StoreError> {
    if capability.kind() != Kind::Application {
        return Err(StoreError::Invalid("Invalid application ownership."));
    }
    Ok(capability.storage_hash().as_slice())
}
fn fields(input: &Submission) -> serde_json::Value {
    serde_json::json!({"version":1,"firstname":input.identity.firstname,
        "handle":input.identity.handle,"email":input.identity.email.recipient(),
        "age":input.review.age,"timezone":input.review.timezone,"hours":input.review.hours,
        "times":input.review.times,"board1":input.review.board1,
        "board2":input.review.board2.as_deref().unwrap_or(""),"answers":input.review.answers})
}
pub async fn submit(
    pool: &PgPool,
    campaign: i64,
    capability: &Capability,
    input: &Submission,
    proof: &Proof<'_>,
) -> Result<i64, StoreError> {
    let id: i64 =
        sqlx::query_scalar("SELECT submission_api.submit_application($1,$2,$3,$4,$5,$6,$7)")
            .bind(campaign)
            .bind(owner(capability)?)
            .bind(fields(input))
            .bind(proof.session.as_slice())
            .bind(proof.receipt.as_slice())
            .bind(proof.peer.as_slice())
            .bind(proof.context.as_slice())
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    if id <= 0 {
        return Err(protocol());
    }
    Ok(id)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: i64,
    pub campaign: i64,
}
pub async fn target(pool: &PgPool, capability: &Capability) -> Result<Option<Target>, StoreError> {
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.application_target($1)")
            .bind(owner(capability)?)
            .fetch_one(pool)
            .await?;
    raw.map(|value| {
        let target: Target = decode(value)?;
        if target.id <= 0 || target.campaign <= 0 {
            return Err(protocol());
        }
        Ok(target)
    })
    .transpose()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApplication {
    id: i64,
    campaign: i64,
    revision: i32,
    firstname: String,
    handle: String,
    email: String,
    age: u16,
    timezone: i16,
    hours: u8,
    times: String,
    board1: String,
    board2: String,
    answers: [String; 4],
}
pub struct Editable {
    pub target: Target,
    pub revision: i32,
    pub fields: Submission,
}
pub async fn authenticate(
    pool: &PgPool,
    capability: &Capability,
    email: &ContactEmail,
    profile: &super::ApplicationProfile,
    proof: &Proof<'_>,
) -> Result<Option<Editable>, StoreError> {
    let raw: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT submission_api.authenticate_application($1,$2,$3,$4,$5,$6)")
            .bind(owner(capability)?)
            .bind(email.recipient())
            .bind(proof.session.as_slice())
            .bind(proof.receipt.as_slice())
            .bind(proof.peer.as_slice())
            .bind(proof.context.as_slice())
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    raw.map(|value| {
        let raw: RawApplication = decode(value)?;
        if raw.id <= 0 || raw.campaign != profile.id || !(1..=1_000_000).contains(&raw.revision) {
            return Err(protocol());
        }
        let fields = Submission::prepare(
            &profile.form,
            applications::Input {
                firstname: &raw.firstname,
                handle: &raw.handle,
                email: &raw.email,
                age: raw.age,
                timezone: raw.timezone,
                hours: raw.hours,
                times: &raw.times,
                board1: &raw.board1,
                board2: &raw.board2,
                answers: raw.answers.each_ref().map(String::as_str),
            },
        )
        .map_err(|_| protocol())?;
        Ok(Editable {
            target: Target {
                id: raw.id,
                campaign: raw.campaign,
            },
            revision: raw.revision,
            fields,
        })
    })
    .transpose()
}
pub async fn edit(
    pool: &PgPool,
    capability: &Capability,
    original_email: &ContactEmail,
    revision: i32,
    input: &Submission,
    proof: &Proof<'_>,
) -> Result<Option<i32>, StoreError> {
    let result: Option<i32> =
        sqlx::query_scalar("SELECT submission_api.edit_application($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(owner(capability)?)
            .bind(original_email.recipient())
            .bind(revision)
            .bind(fields(input))
            .bind(proof.session.as_slice())
            .bind(proof.receipt.as_slice())
            .bind(proof.peer.as_slice())
            .bind(proof.context.as_slice())
            .fetch_one(pool)
            .await
            .map_err(refusal)?;
    if result.is_some_and(|v| !(2..=1_000_000).contains(&v)) {
        return Err(protocol());
    }
    Ok(result)
}
fn refusal(error: sqlx::Error) -> StoreError {
    if let Some(database) = error.as_database_error() {
        if database.code().as_deref() == Some("P0002") {
            return StoreError::Invalid("Applications are closed.");
        }
        if database.code().as_deref() == Some("P0166") {
            return StoreError::Invalid("Submission verification is required.");
        }
        if database.code().as_deref() == Some("P225J") {
            for message in [
                "Invalid application.",
                "Applications are closed.",
                "Please wait before submitting another application.",
                "This application could not be saved.",
            ] {
                if database.message() == message {
                    return StoreError::Invalid(message);
                }
            }
        }
    }
    StoreError::Database(error)
}
