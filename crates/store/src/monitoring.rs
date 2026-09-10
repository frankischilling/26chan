//! One restricted aggregate read; no job identifiers or mutation authority.

use crate::StoreError;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct QueueSnapshot {
    pub capacity: i64,
    pub receiving: i64,
    pub queued: i64,
    pub processing: i64,
    pub expired_receiving: i64,
    pub expired_queued: i64,
    pub expired_processing: i64,
    pub oldest_queued_seconds: i64,
    pub intake_failed: i64,
    pub abandoned: i64,
    pub processing_failed: i64,
    pub invalid_output: i64,
    pub retry_exhausted: i64,
}

impl QueueSnapshot {
    fn validate(self) -> Result<Self, StoreError> {
        if !(1..=1024).contains(&self.capacity)
            || [
                self.receiving,
                self.queued,
                self.processing,
                self.expired_receiving,
                self.expired_queued,
                self.expired_processing,
                self.oldest_queued_seconds,
                self.intake_failed,
                self.abandoned,
                self.processing_failed,
                self.invalid_output,
                self.retry_exhausted,
            ]
            .iter()
            .any(|value| *value < 0)
            || self.expired_receiving > self.receiving
            || self.expired_queued > self.queued
            || self.expired_processing > self.processing
        {
            return Err(StoreError::Invalid("Invalid monitoring aggregate."));
        }
        Ok(self)
    }
}

pub struct MonitorReader {
    pool: PgPool,
}

impl MonitorReader {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(1))
            .connect(url)
            .await?;
        let validation = tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query_scalar::<_, bool>(SAFE_ROLE).fetch_one(&pool),
        )
        .await;
        match validation {
            Ok(Ok(true)) => Ok(Self { pool }),
            Ok(Ok(false)) => {
                pool.close().await;
                Err(StoreError::UnsafeRole)
            }
            Ok(Err(error)) => {
                pool.close().await;
                Err(error.into())
            }
            Err(_) => {
                pool.close().await;
                Err(StoreError::Invalid("Monitoring role validation timed out."))
            }
        }
    }

    pub async fn snapshot(&self) -> Result<QueueSnapshot, StoreError> {
        let snapshots: Vec<QueueSnapshot> = tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query_as(
                "SELECT capacity, receiving, queued, processing, expired_receiving, expired_queued,
             expired_processing, oldest_queued_seconds, intake_failed, abandoned,
             processing_failed, invalid_output, retry_exhausted FROM monitoring.media_queue LIMIT 2",
            )
            .fetch_all(&self.pool),
        )
        .await
        .map_err(|_| StoreError::Invalid("Monitoring snapshot timed out."))??;
        let [snapshot]: [QueueSnapshot; 1] = snapshots.try_into().map_err(|_| {
            StoreError::Invalid("Monitoring aggregate must contain exactly one row.")
        })?;
        snapshot.validate()
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

const SAFE_ROLE: &str = "
SELECT current_user = 'board_monitor' AND session_user = 'board_monitor'
 AND rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
 AND NOT rolreplication AND NOT rolbypassrls
 AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member = role.oid)
 AND NOT EXISTS (SELECT 1 FROM pg_database WHERE datdba = role.oid)
 AND NOT has_database_privilege(current_user, current_database(), 'CREATE,TEMPORARY')
 AND NOT EXISTS (SELECT 1 FROM pg_namespace WHERE nspowner = role.oid)
 AND NOT EXISTS (SELECT 1 FROM pg_class WHERE relowner = role.oid)
 AND NOT EXISTS (SELECT 1 FROM pg_proc WHERE proowner = role.oid)
 AND NOT EXISTS (SELECT 1 FROM pg_type WHERE typowner = role.oid)
 AND NOT EXISTS (SELECT 1 FROM pg_namespace n WHERE has_schema_privilege(current_user,n.oid,'CREATE'))
 AND NOT EXISTS (
   SELECT 1 FROM pg_namespace n
   WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
   AND n.nspname <> 'monitoring' AND has_schema_privilege(current_user,n.oid,'USAGE'))
 AND has_schema_privilege(current_user, 'monitoring', 'USAGE')
 AND has_table_privilege(current_user, 'monitoring.media_queue', 'SELECT')
 AND EXISTS (SELECT 1 FROM pg_class c WHERE c.oid = 'monitoring.media_queue'::regclass
   AND c.relkind = 'v' AND c.relowner = (SELECT oid FROM pg_roles WHERE rolname = 'board_migrator')
   AND 'security_barrier=true' = ANY(c.reloptions)
   AND NOT COALESCE('security_invoker=true' = ANY(c.reloptions),false))
 AND NOT EXISTS (
   SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
   AND c.relkind IN ('r','p','v','m','f') AND (
     (c.oid <> 'monitoring.media_queue'::regclass
      AND (has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
           OR has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
     OR (c.oid = 'monitoring.media_queue'::regclass
      AND (has_table_privilege(current_user,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
           OR has_any_column_privilege(current_user,c.oid,'INSERT,UPDATE,REFERENCES')))))
 AND NOT EXISTS (
   SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE c.relkind='S' AND left(n.nspname,3) <> 'pg_'
   AND has_sequence_privilege(current_user,c.oid,'USAGE,SELECT,UPDATE'))
 AND NOT EXISTS (
   SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
   WHERE left(n.nspname,3) <> 'pg_' AND n.nspname <> 'information_schema'
   AND has_schema_privilege(current_user,n.oid,'USAGE')
   AND has_function_privilege(current_user,p.oid,'EXECUTE'))
 AND EXISTS (SELECT 1 FROM pg_settings WHERE name='statement_timeout' AND setting::bigint BETWEEN 1 AND 2000)
FROM pg_roles AS role WHERE rolname=current_user";
