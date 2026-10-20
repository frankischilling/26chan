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
    pub async fn expired_pass_contact(
        &self,
        identity: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, StoreError> {
        sqlx::query_scalar("SELECT content.pass_expired_contact($1)")
            .bind(identity.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
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

pub struct PurchaseOrder<'a> {
    pub id: &'a str,
    pub view: &'a [u8; 32],
    pub peer: &'a [u8; 32],
    pub recipient: &'a [u8; 32],
    pub amount: i64,
    pub currency: &'a str,
    pub object: &'a str,
    pub delayed: bool,
    pub ticket: &'a [u8; 32],
}
pub struct PurchasePreparation<'a> {
    pub id: &'a str,
    pub ticket: &'a [u8; 32],
    pub identity: &'a [u8; 32],
    pub pin: &'a str,
    pub contact: &'a [u8],
    pub reference: &'a [u8; 32],
    pub setup: &'a [u8; 32],
    pub mail: &'a [u8],
}
impl VerificationStore {
    pub async fn purchase_offer(&self) -> Result<Offer, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_purchase_offer()")
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn begin_purchase(&self, input: &PurchaseOrder<'_>) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "SELECT content.pass_begin_purchase($1::text::uuid,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(input.id)
        .bind(input.view.as_slice())
        .bind(input.peer.as_slice())
        .bind(input.recipient.as_slice())
        .bind(input.amount)
        .bind(input.currency)
        .bind(input.object)
        .bind(input.delayed)
        .bind(input.ticket.as_slice())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }
    pub async fn prepare_purchase(
        &self,
        input: &PurchasePreparation<'_>,
    ) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "SELECT content.pass_prepare_purchase($1::text::uuid,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(input.id)
        .bind(input.ticket.as_slice())
        .bind(input.identity.as_slice())
        .bind(input.pin)
        .bind(input.contact)
        .bind(input.reference.as_slice())
        .bind(input.setup.as_slice())
        .bind(input.mail)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }
    pub async fn purchase_contact(
        &self,
        id: &str,
        view: &[u8; 32],
    ) -> Result<Option<Vec<u8>>, StoreError> {
        sqlx::query_scalar("SELECT content.pass_purchase_contact($1::text::uuid,$2)")
            .bind(id)
            .bind(view.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
}
