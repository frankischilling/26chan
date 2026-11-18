//! Public agreement operations scoped to a single invitation.
use crate::{PgPool, StoreError};
use board_domain::site_submissions::ownership::{Capability, Kind};

fn hash(invitation: &Capability) -> Result<&[u8], StoreError> {
    if invitation.kind() != Kind::ApplicationAgreement {
        return Err(StoreError::Invalid("Invalid agreement invitation."));
    }
    Ok(invitation.storage_hash())
}

pub async fn terms(pool: &PgPool, invitation: &Capability) -> Result<Option<String>, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT submission_api.application_agreement($1)")
            .bind(hash(invitation)?)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn accept(pool: &PgPool, invitation: &Capability) -> Result<bool, StoreError> {
    Ok(
        sqlx::query_scalar("SELECT submission_api.accept_application_agreement($1)")
            .bind(hash(invitation)?)
            .fetch_one(pool)
            .await?,
    )
}
