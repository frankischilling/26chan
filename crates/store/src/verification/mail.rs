//! Delivery-only database access. This role cannot mint verification requests,
//! answer challenges, read browser state, or change account identity.
use crate::StoreError;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone)]
pub struct MailStore {
    pool: PgPool,
}

/// Contains encrypted delivery material. Deliberately has no Debug/Serialize.
#[derive(sqlx::FromRow)]
pub struct MailJob {
    pub request_id: String,
    pub token_hash: Vec<u8>,
    pub encrypted_payload: Vec<u8>,
}

impl MailStore {
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
        let safe: bool = sqlx::query_scalar(
            "SELECT current_user='board_mailer' AND session_user='board_mailer'
             AND content.pass_mail_ready()
             AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
             AND NOT rolreplication AND NOT rolbypassrls
             AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member=pg_roles.oid)
             AND NOT EXISTS(SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=pg_roles.oid)
             AND NOT has_schema_privilege(current_user,'staff_identity','USAGE')
             AND NOT has_schema_privilege(current_user,'deployment','USAGE')
             AND NOT has_schema_privilege(current_user,'post_secrets','USAGE')
             AND NOT has_schema_privilege(current_user,'verification_private','USAGE')
             AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
               WHERE n.nspname='verification_private' AND c.relkind='r'
                 AND has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
             AND coalesce((SELECT count(*)=2 AND bool_and(p.prosecdef AND p.proowner='board_anonymous_owner'::regrole
               AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
               AND has_function_privilege(current_user,p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_public',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_verifier',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_staff',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_auth',p.oid,'EXECUTE'))
               FROM pg_proc p WHERE p.oid IN (
                 to_regprocedure('content.verification_claim_mail(bytea,integer)'),
                 to_regprocedure('content.verification_finish_mail(text,bytea,boolean)'))),false)
             FROM pg_roles WHERE rolname=current_user",
        )
        .fetch_one(&self.pool)
        .await?;
        if safe {
            Ok(())
        } else {
            Err(StoreError::UnsafeRole)
        }
    }

    pub async fn claim(
        &self,
        lease_hash: &[u8; 32],
        limit: i32,
    ) -> Result<Vec<MailJob>, StoreError> {
        sqlx::query_as("SELECT * FROM content.verification_claim_mail($1,$2)")
            .bind(lease_hash.as_slice())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn finish(
        &self,
        request: &str,
        lease_hash: &[u8; 32],
        success: bool,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.verification_finish_mail($1,$2,$3)")
            .bind(request)
            .bind(lease_hash.as_slice())
            .bind(success)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn claim_recovery(
        &self,
        lease: &[u8; 32],
        limit: i32,
    ) -> Result<Vec<MailJob>, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_claim_recovery_mail($1,$2)")
            .bind(lease.as_slice())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn finish_recovery(
        &self,
        request: &str,
        lease: &[u8; 32],
        success: bool,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_finish_recovery_mail($1,$2,$3)")
            .bind(request)
            .bind(lease.as_slice())
            .bind(success)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn claim_pass(
        &self,
        lease_hash: &[u8; 32],
        limit: i32,
    ) -> Result<Vec<MailJob>, StoreError> {
        sqlx::query_as("SELECT * FROM content.pass_claim_mail($1,$2)")
            .bind(lease_hash.as_slice())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from)
    }
    pub async fn finish_pass(
        &self,
        request: &str,
        lease_hash: &[u8; 32],
        success: bool,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.pass_finish_mail($1,$2,$3)")
            .bind(request)
            .bind(lease_hash.as_slice())
            .bind(success)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
