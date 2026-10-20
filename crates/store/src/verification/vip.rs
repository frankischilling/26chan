//! Isolated VIP issuer access; no public credential or table reader.
use super::VerificationStore;
use crate::StoreError;

#[derive(sqlx::FromRow)]
pub struct Lookup {
    pub status: String,
    pub password_hash: Option<String>,
}

pub struct LoginRequest<'a> {
    pub identifier: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub ticket: &'a [u8; 32],
    pub anonymous: &'a [u8; 32],
    pub context: &'a [u8; 32],
    pub board: &'a str,
    pub target: i64,
    pub pass_proof: Option<(&'a [u8; 32], &'a [u8; 32])>,
}

impl VerificationStore {
    pub async fn vip_ready(&self) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar("SELECT content.public_vip_ready()")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn begin_vip_login(&self, input: &LoginRequest<'_>) -> Result<Lookup, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM content.vip_begin_login($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(input.identifier.as_slice())
                .bind(input.peer.as_slice())
                .bind(input.ticket.as_slice())
                .bind(input.anonymous.as_slice())
                .bind(input.context.as_slice())
                .bind(input.board)
                .bind(input.target)
                .bind(input.pass_proof.map(|(receipt, _)| receipt.as_slice()))
                .bind(input.pass_proof.map(|(_, context)| context.as_slice()))
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn finish_vip_login(
        &self,
        ticket: &[u8; 32],
        peer: &[u8; 32],
        verified: bool,
        receipt: &[u8; 32],
    ) -> Result<String, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT content.vip_finish_login($1,$2,$3,$4)")
                .bind(ticket.as_slice())
                .bind(peer.as_slice())
                .bind(verified)
                .bind(receipt.as_slice())
                .fetch_one(&self.pool)
                .await?,
        )
    }
}
