//! Queue authority only. This module does not open uploads or execute workers.
use crate::StoreError;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone, sqlx::FromRow)]
pub struct Job {
    pub id: String,
    pub filename: String,
    pub state: String,
    pub input_bytes: Option<i64>,
    pub attempts: i32,
    pub lease_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub output_sha256: Option<String>,
    pub output_bytes: Option<i64>,
    pub failure: Option<String>,
}

#[derive(Clone, Copy)]
pub enum Failure {
    Processing,
    InvalidOutput,
}

#[derive(Clone)]
pub struct MediaQueue {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
struct CompletionRecord {
    state: String,
    lease_token: Option<String>,
    output_sha256: Option<String>,
    output_bytes: Option<i64>,
}

impl MediaQueue {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(3))
            .connect(url)
            .await?;
        let safe: bool = sqlx::query_scalar("SELECT current_user = 'board_media' AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member = pg_roles.oid) AND NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = current_database() AND datdba = pg_roles.oid) AND NOT has_schema_privilege(current_user, 'content', 'USAGE') AND NOT has_schema_privilege(current_user, 'post_secrets', 'USAGE') AND NOT has_schema_privilege(current_user, 'staff_identity', 'USAGE') AND NOT has_schema_privilege(current_user, 'deployment', 'USAGE') FROM pg_roles WHERE rolname = current_user").fetch_one(&pool).await?;
        if !safe {
            pool.close().await;
            return Err(StoreError::UnsafeRole);
        }
        Ok(Self { pool })
    }

    pub async fn reserve(&self, filename: &str) -> Result<Job, StoreError> {
        if filename.is_empty() || filename.len() > 255 || filename.chars().any(char::is_control) {
            return Err(StoreError::Invalid(
                "Filename must contain 1 to 255 bytes without control characters.",
            ));
        }
        let mut tx = self.pool.begin().await?;
        let capacity: i32 = sqlx::query_scalar(
            "SELECT capacity FROM media.queue_policy WHERE singleton FOR UPDATE",
        )
        .fetch_one(&mut *tx)
        .await?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM media.jobs WHERE state IN ('receiving', 'queued', 'processing')",
        )
        .fetch_one(&mut *tx)
        .await?;
        if pending >= i64::from(capacity) {
            return Err(StoreError::Conflict(
                "The media queue is full. Try again later.",
            ));
        }
        // PostgreSQL generates opaque random identifiers; no client value selects an object path.
        let job = sqlx::query_as::<_, Job>("INSERT INTO media.jobs (id, filename, expires_at) VALUES (replace(gen_random_uuid()::text, '-', ''), $1, clock_timestamp() + interval '5 minutes') RETURNING *")
            .bind(filename).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(job)
    }

    pub async fn get(&self, id: &str) -> Result<Job, StoreError> {
        validate_hex(id, 32)?;
        sqlx::query_as("SELECT * FROM media.jobs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)
    }

    pub async fn queue(&self, id: &str, input_bytes: u64) -> Result<(), StoreError> {
        validate_hex(id, 32)?;
        if !(1..=8_388_608).contains(&input_bytes) {
            return Err(StoreError::Invalid(
                "Upload size is outside the permitted range.",
            ));
        }
        changed(sqlx::query("UPDATE media.jobs SET state = 'queued', input_bytes = $2, expires_at = clock_timestamp() + interval '1 hour', updated_at = clock_timestamp() WHERE id = $1 AND state = 'receiving' AND expires_at > clock_timestamp()")
            .bind(id).bind(input_bytes as i64).execute(&self.pool).await?.rows_affected())
    }

    pub async fn abort_intake(&self, id: &str) -> Result<(), StoreError> {
        validate_hex(id, 32)?;
        changed(sqlx::query("UPDATE media.jobs SET state = 'failed', failure = 'intake_failed', expires_at = NULL, updated_at = clock_timestamp() WHERE id = $1 AND state = 'receiving'")
            .bind(id).execute(&self.pool).await?.rows_affected())
    }

    pub async fn claim(&self) -> Result<Option<Job>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let id: Option<String> = sqlx::query_scalar("SELECT id FROM media.jobs WHERE state = 'queued' AND attempts < 3 AND expires_at > clock_timestamp() ORDER BY created_at, id FOR UPDATE SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(id) = id else {
            return Ok(None);
        };
        let job = sqlx::query_as("UPDATE media.jobs SET state = 'processing', attempts = attempts + 1, lease_token = replace(gen_random_uuid()::text, '-', ''), expires_at = clock_timestamp() + interval '30 seconds', updated_at = clock_timestamp() WHERE id = $1 RETURNING *")
            .bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(job))
    }

    /// Record a trusted promoter receipt. This is not authority to publish worker-supplied bytes.
    pub async fn complete(
        &self,
        id: &str,
        token: &str,
        digest: &str,
        bytes: u64,
    ) -> Result<(), StoreError> {
        validate_hex(id, 32)?;
        validate_hex(token, 32)?;
        validate_hex(digest, 64)?;
        if !(1..=5_242_880).contains(&bytes) {
            return Err(StoreError::Invalid(
                "Output size is outside the permitted range.",
            ));
        }
        let mut tx = self.pool.begin().await?;
        let current: Option<CompletionRecord> = sqlx::query_as("SELECT state, lease_token, output_sha256, output_bytes FROM media.jobs WHERE id = $1 FOR UPDATE")
            .bind(id).fetch_optional(&mut *tx).await?;
        if let Some(current) = current
            && current.state == "published"
            && current.lease_token.as_deref() == Some(token)
            && current.output_sha256.as_deref() == Some(digest)
            && current.output_bytes == Some(bytes as i64)
        {
            tx.commit().await?;
            return Ok(());
        }
        changed(sqlx::query("UPDATE media.jobs SET state = 'published', output_sha256 = $3, output_bytes = $4, expires_at = NULL, updated_at = clock_timestamp() WHERE id = $1 AND state = 'processing' AND lease_token = $2 AND expires_at > clock_timestamp()")
            .bind(id).bind(token).bind(digest).bind(bytes as i64).execute(&mut *tx).await?.rows_affected())?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn fail(
        &self,
        id: &str,
        token: &str,
        failure: Failure,
        retryable: bool,
    ) -> Result<(), StoreError> {
        validate_hex(id, 32)?;
        validate_hex(token, 32)?;
        let failure = match failure {
            Failure::Processing => "processing_failed",
            Failure::InvalidOutput => "invalid_output",
        };
        changed(sqlx::query("UPDATE media.jobs SET state = CASE WHEN $4 AND attempts < 3 THEN 'queued' ELSE 'failed' END, failure = CASE WHEN $4 AND attempts < 3 THEN NULL WHEN $4 THEN 'retry_exhausted' ELSE $3 END, lease_token = NULL, expires_at = CASE WHEN $4 AND attempts < 3 THEN clock_timestamp() + interval '1 hour' ELSE NULL END, updated_at = clock_timestamp() WHERE id = $1 AND state = 'processing' AND lease_token = $2 AND expires_at > clock_timestamp()")
            .bind(id).bind(token).bind(failure).bind(retryable).execute(&self.pool).await?.rows_affected())
    }

    /// Bounded reconciliation; rerun while it returns 64 to drain a larger backlog.
    pub async fn expire(&self) -> Result<u64, StoreError> {
        Ok(sqlx::query("WITH expired AS (SELECT id FROM media.jobs WHERE expires_at <= clock_timestamp() ORDER BY expires_at, id FOR UPDATE SKIP LOCKED LIMIT 64) UPDATE media.jobs j SET state = CASE WHEN j.state = 'processing' AND attempts < 3 THEN 'queued' ELSE 'failed' END, failure = CASE WHEN j.state = 'processing' AND attempts < 3 THEN NULL WHEN j.state = 'processing' THEN 'retry_exhausted' ELSE 'abandoned' END, expires_at = CASE WHEN j.state = 'processing' AND attempts < 3 THEN clock_timestamp() + interval '1 hour' ELSE NULL END, lease_token = NULL, updated_at = clock_timestamp() FROM expired WHERE j.id = expired.id")
            .execute(&self.pool).await?.rows_affected())
    }

    pub async fn cleanup_candidates(&self) -> Result<Vec<Job>, StoreError> {
        Ok(sqlx::query_as("SELECT * FROM media.jobs WHERE state IN ('published', 'failed') AND updated_at < clock_timestamp() - interval '1 day' ORDER BY updated_at, id LIMIT 64")
            .fetch_all(&self.pool).await?)
    }

    /// Delete metadata only after private-file removal succeeds. Never removes public artifacts.
    pub async fn forget_terminal(&self, id: &str) -> Result<bool, StoreError> {
        validate_hex(id, 32)?;
        Ok(sqlx::query("DELETE FROM media.jobs WHERE id = $1 AND state IN ('failed', 'published') AND updated_at < clock_timestamp() - interval '1 day'")
            .bind(id).execute(&self.pool).await?.rows_affected() == 1)
    }
}

fn validate_hex(value: &str, length: usize) -> Result<(), StoreError> {
    if value.len() != length
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(StoreError::Invalid("Invalid media identifier or digest."));
    }
    Ok(())
}

fn changed(rows: u64) -> Result<(), StoreError> {
    if rows == 1 {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "Media job state or lease is no longer valid.",
        ))
    }
}
