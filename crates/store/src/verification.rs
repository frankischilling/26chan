//! Restricted verification issuer access. Never construct this with public,
//! staff, migrator or mail-sink credentials.
pub mod mail;

use crate::{StoreError, anonymous_session::Snapshot};
use board_domain::anonymous_session::Fingerprints;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone)]
pub struct VerificationStore {
    pool: PgPool,
}

#[derive(Clone, Copy)]
pub enum ChallengePurpose {
    Thread,
    Reply,
    Report,
    EmailRequest,
    EmailVerify,
    Ticket,
}
impl ChallengePurpose {
    fn value(self) -> &'static str {
        match self {
            Self::Thread => "thread",
            Self::Reply => "reply",
            Self::Report => "report",
            Self::EmailRequest => "email_request",
            Self::EmailVerify => "email_verify",
            Self::Ticket => "ticket",
        }
    }
}

pub struct NewChallenge<'a> {
    pub token_hash: &'a [u8; 32],
    pub answer_hash: &'a [u8; 32],
    pub peer_hash: &'a [u8; 32],
    pub session_hash: &'a [u8; 32],
    pub purpose: ChallengePurpose,
    pub board: Option<&'a str>,
    pub target: i64,
    pub suspicious: bool,
    pub context_hash: &'a [u8; 32],
}

#[derive(sqlx::FromRow)]
pub struct ChallengeIssuance {
    pub issued: bool,
    pub cooldown_seconds: i32,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct ChallengeCheck<'a> {
    pub token_hash: &'a [u8; 32],
    /// None represents a malformed answer and still consumes this challenge.
    pub answer_hash: Option<&'a [u8; 32]>,
    pub peer_hash: &'a [u8; 32],
    pub session_hash: &'a [u8; 32],
    pub purpose: ChallengePurpose,
    pub board: Option<&'a str>,
    pub target: i64,
    pub context_hash: &'a [u8; 32],
    pub receipt_hash: &'a [u8; 32],
}

pub struct EmailRequest<'a> {
    pub token_hash: &'a [u8; 32],
    pub email_hash: &'a [u8; 32],
    pub recipient_hash: &'a [u8; 32],
    pub peer_hash: &'a [u8; 32],
    pub session_hash: &'a [u8; 32],
    pub country: &'a str,
    pub encrypted_payload: &'a [u8],
    pub receipt_hash: &'a [u8; 32],
}

#[derive(sqlx::FromRow)]
pub struct EmailQueueResult {
    pub status: String,
    pub cooldown_seconds: i32,
}

impl VerificationStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
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
            "SELECT current_user='board_verifier' AND session_user='board_verifier'
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
             AND coalesce((SELECT count(*)=4 AND bool_and(p.prosecdef AND p.proowner='board_anonymous_owner'::regrole
               AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
               AND has_function_privilege(current_user,p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_public',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_mailer',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_staff',p.oid,'EXECUTE')
               AND NOT has_function_privilege('board_auth',p.oid,'EXECUTE'))
               FROM pg_proc p WHERE p.oid IN (
                 to_regprocedure('content.verification_ensure_session(bytea,bytea,bytea,bytea,boolean)'),
                 to_regprocedure('content.verification_issue_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,boolean)'),
                 to_regprocedure('content.verification_check_challenge(bytea,bytea,bytea,bytea,text,text,bigint,bytea,bytea)'),
                 to_regprocedure('content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea)'))),false)
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

    /// Persist only a freshly server-minted guest or validate an existing one.
    /// This operation never increments posting/report activity or verification.
    pub async fn ensure_session(
        &self,
        fingerprints: &Fingerprints,
        minted: bool,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.verification_ensure_session($1,$2,$3,$4,$5)")
            .bind(fingerprints.token.as_slice())
            .bind(fingerprints.network.as_slice())
            .bind(fingerprints.address.as_slice())
            .bind(fingerprints.environment.as_slice())
            .bind(minted)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn session(&self, token_hash: &[u8; 32]) -> Result<Option<Snapshot>, StoreError> {
        crate::anonymous_session::snapshot(&self.pool, token_hash).await
    }

    pub async fn issue_challenge(
        &self,
        input: &NewChallenge<'_>,
    ) -> Result<ChallengeIssuance, StoreError> {
        sqlx::query_as(
            "SELECT * FROM content.verification_issue_challenge($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(input.token_hash.as_slice())
        .bind(input.answer_hash.as_slice())
        .bind(input.peer_hash.as_slice())
        .bind(input.session_hash.as_slice())
        .bind(input.purpose.value())
        .bind(input.board)
        .bind(input.target)
        .bind(input.context_hash.as_slice())
        .bind(input.suspicious)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn check_challenge(&self, input: &ChallengeCheck<'_>) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            "SELECT content.verification_check_challenge($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(input.token_hash.as_slice())
        .bind(input.answer_hash.map(|hash| hash.as_slice()))
        .bind(input.peer_hash.as_slice())
        .bind(input.session_hash.as_slice())
        .bind(input.purpose.value())
        .bind(input.board)
        .bind(input.target)
        .bind(input.context_hash.as_slice())
        .bind(input.receipt_hash.as_slice())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn queue_email(
        &self,
        input: &EmailRequest<'_>,
    ) -> Result<EmailQueueResult, StoreError> {
        sqlx::query_as("SELECT * FROM content.verification_queue_email($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(input.token_hash.as_slice())
            .bind(input.email_hash.as_slice())
            .bind(input.recipient_hash.as_slice())
            .bind(input.peer_hash.as_slice())
            .bind(input.session_hash.as_slice())
            .bind(input.country)
            .bind(input.encrypted_payload)
            .bind(input.receipt_hash.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
