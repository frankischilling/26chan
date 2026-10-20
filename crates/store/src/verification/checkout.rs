//! The verifier can quote and create orders, but cannot acknowledge payment.
use super::VerificationStore;
use crate::StoreError;
#[derive(sqlx::FromRow)]
pub struct Offer {
    pub enabled: bool,
    pub provider: String,
    pub amount_minor: i64,
    pub currency: String,
}
#[derive(sqlx::FromRow)]
pub struct OrderState {
    pub state: String,
    pub provider: String,
    pub product: String,
    pub amount_minor: i64,
    pub currency: String,
    pub fulfilled: bool,
    pub review_required: bool,
}
pub struct RenewalOrder<'a> {
    pub id: &'a str,
    pub view: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub reference: &'a [u8; 32],
    pub amount: i64,
    pub currency: &'a str,
    pub object: &'a str,
}
impl VerificationStore {
    pub async fn checkout_offer(&self) -> Result<Offer, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_checkout_offer()")
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn bind_pass_renewal(
        &self,
        identifier: &[u8; 32],
        recipient: &[u8; 32],
        reference: &[u8; 32],
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_bind_renewal($1,$2,$3)")
            .bind(identifier.as_slice())
            .bind(recipient.as_slice())
            .bind(reference.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn create_renewal_order(
        &self,
        order: &RenewalOrder<'_>,
    ) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "SELECT content.pass_create_renewal_order($1::text::uuid,$2,$3,$4,$5,$6,$7)",
        )
        .bind(order.id)
        .bind(order.view.as_slice())
        .bind(order.peer.as_slice())
        .bind(order.reference.as_slice())
        .bind(order.amount)
        .bind(order.currency)
        .bind(order.object)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }
    pub async fn checkout_state(
        &self,
        id: &str,
        view: &[u8; 32],
    ) -> Result<Option<OrderState>, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_checkout_state($1::text::uuid,$2)")
            .bind(id)
            .bind(view.as_slice())
            .fetch_optional(&self.pool)
            .await
            .map_err(StoreError::from)
    }
}
