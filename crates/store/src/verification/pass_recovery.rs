//! Private recovery preparation and atomic credential rotation.
use super::VerificationStore;
use crate::StoreError;
#[derive(sqlx::FromRow)]
pub struct RecoveryPreparation {
    pub status: String,
    pub encrypted_contact: Option<Vec<u8>>,
}
impl VerificationStore {
    pub async fn begin_pass_recovery(
        &self,
        identifier: &[u8; 32],
        recipient: &[u8; 32],
        peer: &[u8; 32],
        ticket: &[u8; 32],
    ) -> Result<RecoveryPreparation, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_begin_recovery($1,$2,$3,$4)")
            .bind(identifier.as_slice())
            .bind(recipient.as_slice())
            .bind(peer.as_slice())
            .bind(ticket.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn queue_pass_recovery(
        &self,
        ticket: &[u8; 32],
        token: &[u8; 32],
        request: &str,
        payload: &[u8],
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_queue_recovery($1,$2,$3::text::uuid,$4)")
            .bind(ticket.as_slice())
            .bind(token.as_slice())
            .bind(request)
            .bind(payload)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn pass_recovery_state(&self, token: &[u8; 32]) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_recovery_state($1)")
            .bind(token.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn complete_pass_recovery(
        &self,
        token: &[u8; 32],
        pin: &str,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_complete_recovery($1,$2)")
            .bind(token.as_slice())
            .bind(pin)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
}
