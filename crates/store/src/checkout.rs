//! Payment adapters can apply authenticated events, never read contact records.
use crate::StoreError;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
#[derive(Clone)]
pub struct CheckoutStore {
    pool: PgPool,
}
pub struct PaymentEvent<'a> {
    pub provider: &'a str,
    pub event: &'a str,
    pub order: &'a str,
    pub object: &'a str,
    pub product: &'a str,
    pub amount: i64,
    pub currency: &'a str,
    pub kind: &'a str,
    pub occurred: DateTime<Utc>,
    pub fingerprint: &'a [u8; 32],
}
impl CheckoutStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(3))
            .connect(url)
            .await?;
        let store = Self { pool };
        if let Err(error) = store.ready().await {
            store.pool.close().await;
            return Err(error);
        }
        Ok(store)
    }
    pub async fn ready(&self) -> Result<(), StoreError> {
        let ready:bool=sqlx::query_scalar("SELECT current_user='board_checkout' AND session_user='board_checkout' AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member=pg_roles.oid) AND NOT EXISTS(SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=pg_roles.oid) AND NOT has_schema_privilege(current_user,'verification_private','USAGE') AND NOT has_schema_privilege(current_user,'staff_identity','USAGE') AND NOT has_schema_privilege(current_user,'deployment','USAGE') AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='verification_private' AND c.relkind='r' AND has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')) FROM pg_roles WHERE rolname=current_user").fetch_one(&self.pool).await?;
        if !ready {
            return Err(StoreError::UnsafeRole);
        }
        let catalog: bool = sqlx::query_scalar("SELECT content.pass_checkout_ready()")
            .fetch_one(&self.pool)
            .await?;
        if catalog {
            Ok(())
        } else {
            Err(StoreError::UnsafeRole)
        }
    }

    pub async fn apply(&self, event: &PaymentEvent<'_>) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "SELECT content.pass_apply_checkout_event($1,$2,$3::text::uuid,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(event.provider)
        .bind(event.event)
        .bind(event.order)
        .bind(event.object)
        .bind(event.product)
        .bind(event.amount)
        .bind(event.currency)
        .bind(event.kind)
        .bind(event.occurred)
        .bind(event.fingerprint.as_slice())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }
    pub async fn close(&self) {
        self.pool.close().await;
    }
}
