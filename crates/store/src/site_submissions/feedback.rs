//! A protected feedback write. Header/peer fields are server-owned facts;
//! the browser never supplies database receipt digests or a quota identity.
use crate::{PgPool, StoreError};
use board_domain::site_submissions::feedback::Submission;
use std::net::IpAddr;

pub struct Proof<'a> {
    pub session: &'a [u8; 32],
    pub receipt: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub context: &'a [u8; 32],
}
pub struct Request<'a> {
    pub profile: i64,
    pub fields: &'a Submission,
    pub address: IpAddr,
    pub user_agent: &'a str,
    pub language: &'a str,
    pub proof: Proof<'a>,
}
pub async fn submit(pool: &PgPool, input: &Request<'_>) -> Result<i64, StoreError> {
    let id: i64 = sqlx::query_scalar(
        "SELECT submission_api.submit_feedback($1,$2,$3,$4,$5::inet,$6,$7,$8,$9,$10,$11)",
    )
    .bind(input.profile)
    .bind(i32::from(input.fields.category))
    .bind(&input.fields.subject)
    .bind(&input.fields.message)
    .bind(input.address.to_canonical().to_string())
    .bind(input.user_agent)
    .bind(input.language)
    .bind(input.proof.session.as_slice())
    .bind(input.proof.receipt.as_slice())
    .bind(input.proof.peer.as_slice())
    .bind(input.proof.context.as_slice())
    .fetch_one(pool)
    .await
    .map_err(refusal)?;
    if id <= 0 {
        return Err(super::protocol());
    }
    Ok(id)
}
fn refusal(error: sqlx::Error) -> StoreError {
    if let Some(database) = error.as_database_error() {
        if database.code().as_deref() == Some("P0166") {
            return StoreError::Invalid("Submission verification is required.");
        }
        if database.code().as_deref() == Some("P0002") {
            return StoreError::Invalid("Feedback is closed.");
        }
        if database.code().as_deref() == Some("P0225") {
            for message in [
                "Invalid feedback submission.",
                "Feedback is closed.",
                "Feedback capacity has been reached.",
                "Please wait before submitting more feedback.",
            ] {
                if database.message() == message {
                    return StoreError::Invalid(message);
                }
            }
        }
    }
    StoreError::Database(error)
}
