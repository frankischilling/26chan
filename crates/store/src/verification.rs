//! Restricted verification issuer access. Never construct this with public,
//! staff, migrator or mail-sink credentials.
pub mod mail;

use crate::{StoreError, anonymous_session::Snapshot};
use board_domain::anonymous_session::Fingerprints;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(sqlx::FromRow)]
pub struct VerificationRisk {
    pub current_score: i16,
    pub token_score: i16,
}
impl VerificationRisk {
    pub fn suspicious(&self) -> bool {
        self.current_score < 80 || self.token_score < 80
    }
    pub fn escalated(&self) -> bool {
        self.token_score < 90
    }
}

#[derive(Clone)]
pub struct VerificationStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
pub struct ChallengeBoardPolicy {
    pub enabled: bool,
    pub twister: bool,
    pub allow_bypass: bool,
    pub preupload: bool,
    pub noscript_only: bool,
}

#[derive(sqlx::FromRow)]
pub struct PreparedTicket {
    pub issued: bool,
    pub issued_at: i64,
    pub current_at: i64,
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
    pub context_hash: &'a [u8; 32],
}

#[derive(sqlx::FromRow)]
pub struct EmailQueueResult {
    pub status: String,
    pub cooldown_seconds: i32,
}

pub struct EmailAcceptance<'a> {
    pub token_hash: &'a [u8; 32],
    pub peer_hash: &'a [u8; 32],
    pub country: &'a str,
    pub receipt_hash: &'a [u8; 32],
    pub previous: &'a Fingerprints,
    pub replacement: &'a Fingerprints,
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
             AND coalesce((SELECT count(*)=11 AND bool_and(p.prosecdef AND p.proowner='board_anonymous_owner'::regrole
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
                 to_regprocedure('content.verification_queue_email(bytea,bytea,bytea,bytea,bytea,text,bytea,bytea,bytea)'),
                 to_regprocedure('content.verification_email_state(bytea)'),
                 to_regprocedure('content.verification_accept_email(bytea,bytea,text,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea,bytea)'),
                 to_regprocedure('content.verification_signout(bytea)'),
                 to_regprocedure('content.verification_email_advice(bytea,bytea,bytea)'),
                 to_regprocedure('content.verification_risk(bytea,bytea)'),
                 to_regprocedure('content.verification_board_policy(text)'),
                 to_regprocedure('content.verification_prepare_ticket(bytea,bytea,bytea,text)'))),false)
             AND coalesce((SELECT count(*)=1 AND bool_and(p.prosecdef
                AND p.proowner='board_anonymous_owner'::regrole
                AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
                AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl
                  WHERE acl.privilege_type='EXECUTE' AND acl.grantee<>p.proowner))
                FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
                WHERE n.nspname='verification_private' AND p.proname='email_cooldown'
                  AND p.proargtypes='17 17 17 1184'::oidvector),false)
             AND coalesce((SELECT count(*)=1 AND bool_and(p.prosecdef
                AND p.proowner='board_restriction_owner'::regrole
                AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
                AND has_function_privilege('board_anonymous_owner',p.oid,'EXECUTE')
                AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl
                    WHERE acl.privilege_type='EXECUTE' AND acl.grantee NOT IN(p.proowner,'board_anonymous_owner'::regrole)))
                FROM pg_proc p WHERE p.oid=to_regprocedure('content.verification_identity_banned(uuid)')),false)
             AND coalesce((SELECT count(*)=1 AND bool_and(p.prosecdef
                AND p.proowner='board_anonymous_owner'::regrole
                AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
                AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl
                    WHERE acl.privilege_type='EXECUTE' AND acl.grantee<>p.proowner)
                AND EXISTS(SELECT 1 FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
                    WHERE n.nspname='verification_private' AND c.relname='blocked_recipients'
                    AND t.tgname='verification_recipient_policy_guard' AND t.tgfoid=p.oid
                    AND t.tgenabled='O' AND t.tgtype=31 AND t.tgnargs=0 AND t.tgqual IS NULL AND NOT t.tgisinternal)
                AND EXISTS(SELECT 1 FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
                    WHERE n.nspname='verification_private' AND c.relname='peer_risk'
                    AND t.tgname='verification_peer_risk_guard' AND t.tgfoid=p.oid AND t.tgenabled='O'
                    AND t.tgtype=31 AND t.tgnargs=0 AND t.tgqual IS NULL AND NOT t.tgisinternal))
                FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
                WHERE n.nspname='verification_private' AND p.proname='lock_recipient_policy' AND p.pronargs=0),false)
             AND coalesce((SELECT count(*)=1 AND bool_and(p.prosecdef
                AND p.proowner='board_anonymous_owner'::regrole AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
                AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) acl
                    WHERE acl.privilege_type='EXECUTE' AND acl.grantee<>p.proowner)
                AND EXISTS(SELECT 1 FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace
                    WHERE n.nspname='verification_private' AND c.relname='email_requests'
                    AND t.tgname='verification_email_risk_snapshot' AND t.tgfoid=p.oid AND t.tgenabled='O'
                    AND t.tgtype=7 AND t.tgnargs=0 AND t.tgqual IS NULL AND NOT t.tgisinternal))
                FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
                WHERE n.nspname='verification_private' AND p.proname='snapshot_email_risk' AND p.pronargs=0),false)
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
    pub async fn board_policy(
        &self,
        board: &str,
    ) -> Result<Option<ChallengeBoardPolicy>, StoreError> {
        Ok(
            sqlx::query_as("SELECT * FROM content.verification_board_policy($1)")
                .bind(board)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// None means capacity is exhausted. The issuer must fail closed while
    /// allowing this call's bounded expired-row cleanup to commit.
    pub async fn prepare_ticket(
        &self,
        presented: Option<&[u8; 32]>,
        candidate: &[u8; 32],
        peer: &[u8; 32],
        board: &str,
    ) -> Result<Option<PreparedTicket>, StoreError> {
        Ok(
            sqlx::query_as("SELECT issued,issued_at,current_at FROM content.verification_prepare_ticket($1,$2,$3,$4) WHERE available")
                .bind(presented.map(|value| value.as_slice()))
                .bind(candidate.as_slice())
                .bind(peer.as_slice())
                .bind(board)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

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

    /// Trusted risk for rendering; enqueue snapshots and acceptance rechecks it.
    pub async fn risk(
        &self,
        peer: &[u8; 32],
        token: Option<&[u8; 32]>,
    ) -> Result<VerificationRisk, StoreError> {
        sqlx::query_as("SELECT * FROM content.verification_risk($1,$2)")
            .bind(peer.as_slice())
            .bind(token.map(|v| v.as_slice()))
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    /// Advisory only. Enqueue repeats these checks under its capacity lock.
    pub async fn email_advice(
        &self,
        peer: &[u8; 32],
        email: &[u8; 32],
        recipient: &[u8; 32],
    ) -> Result<EmailQueueResult, StoreError> {
        sqlx::query_as("SELECT * FROM content.verification_email_advice($1,$2,$3)")
            .bind(peer.as_slice())
            .bind(email.as_slice())
            .bind(recipient.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn queue_email(
        &self,
        input: &EmailRequest<'_>,
    ) -> Result<EmailQueueResult, StoreError> {
        sqlx::query_as("SELECT * FROM content.verification_queue_email($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(input.token_hash.as_slice())
            .bind(input.email_hash.as_slice())
            .bind(input.recipient_hash.as_slice())
            .bind(input.peer_hash.as_slice())
            .bind(input.session_hash.as_slice())
            .bind(input.country)
            .bind(input.encrypted_payload)
            .bind(input.receipt_hash.as_slice())
            .bind(input.context_hash.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn email_state(&self, token_hash: &[u8; 32]) -> Result<String, StoreError> {
        sqlx::query_scalar("SELECT content.verification_email_state($1)")
            .bind(token_hash.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn accept_email(&self, input: &EmailAcceptance<'_>) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "SELECT content.verification_accept_email($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(input.token_hash.as_slice())
        .bind(input.peer_hash.as_slice())
        .bind(input.country)
        .bind(input.receipt_hash.as_slice())
        .bind(input.previous.token.as_slice())
        .bind(input.previous.network.as_slice())
        .bind(input.previous.address.as_slice())
        .bind(input.previous.environment.as_slice())
        .bind(input.replacement.token.as_slice())
        .bind(input.replacement.network.as_slice())
        .bind(input.replacement.address.as_slice())
        .bind(input.replacement.environment.as_slice())
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn signout(&self, token_hash: &[u8; 32]) -> Result<bool, StoreError> {
        sqlx::query_scalar("SELECT content.verification_signout($1)")
            .bind(token_hash.as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod risk_tests {
    use super::VerificationRisk;
    #[test]
    fn source_risk_boundaries_are_distinct() {
        for (current, token, suspicious, escalated) in [
            (100, 100, false, false),
            (79, 100, true, false),
            (80, 100, false, false),
            (100, 79, true, true),
            (100, 80, false, true),
            (100, 89, false, true),
            (100, 90, false, false),
        ] {
            let risk = VerificationRisk {
                current_score: current,
                token_score: token,
            };
            assert_eq!(risk.suspicious(), suspicious);
            assert_eq!(risk.escalated(), escalated);
        }
    }
}
