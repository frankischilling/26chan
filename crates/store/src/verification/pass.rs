//! Membership access is confined to the isolated verifier connection.
use super::VerificationStore;
use crate::StoreError;
use board_domain::challenge_secrets::PassNetworks;

#[derive(sqlx::FromRow)]
pub struct Lookup {
    pub status: String,
    pub pin_hash: Option<String>,
}
#[derive(sqlx::FromRow)]
pub struct Login {
    pub status: String,
    pub remaining: i32,
    pub ttl_seconds: i32,
}
#[derive(sqlx::FromRow)]
pub struct State {
    pub status: String,
    pub purchase_year: Option<i32>,
}
impl VerificationStore {
    pub async fn begin_pass_login(
        &self,
        identifier: &[u8; 32],
        peer: &[u8; 32],
        ticket: &[u8; 32],
    ) -> Result<Lookup, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM content.pass_begin_login($1,$2,$3)")
                .bind(identifier.as_slice())
                .bind(peer.as_slice())
                .bind(ticket.as_slice())
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn finish_pass_login(
        &self,
        ticket: &[u8; 32],
        networks: &PassNetworks,
        verified: bool,
        session: &[u8; 32],
        remember: bool,
    ) -> Result<Login, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM content.pass_finish_login($1,$2,$3,$4,$5,$6,$7)")
                .bind(ticket.as_slice())
                .bind(networks.peer.as_slice())
                .bind(networks.login.as_ref().map(|v| v.as_slice()))
                .bind(networks.posting.as_ref().map(|v| v.as_slice()))
                .bind(verified)
                .bind(session.as_slice())
                .bind(remember)
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn pass_state(&self, session: &[u8; 32]) -> Result<State, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM content.pass_session_state($1)")
                .bind(session.as_slice())
                .fetch_one(&self.pool)
                .await?,
        )
    }
    pub async fn pass_signout(&self, session: &[u8; 32]) -> Result<(), StoreError> {
        sqlx::query("SELECT content.pass_signout($1)")
            .bind(session.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
