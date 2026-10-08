//! Typed inactive candidate leases. These cannot complete or publish a media job.
use crate::{
    StoreError,
    media::{Job, MediaQueue},
};
use chrono::{DateTime, Utc};

/// Constructed only by the SQL claim boundary. The token stays in the host.
pub struct PairedCandidateLease {
    job: Job,
}
impl PairedCandidateLease {
    pub fn id(&self) -> &str {
        &self.job.id
    }
    pub fn input_bytes(&self) -> Option<i64> {
        self.job.input_bytes
    }
    pub fn input_sha256(&self) -> Option<&str> {
        self.job.input_sha256.as_deref()
    }
    pub fn image_bytes(&self) -> Option<i64> {
        self.job.input_image_bytes
    }
    pub fn image_sha256(&self) -> Option<&str> {
        self.job.input_image_sha256.as_deref()
    }
    pub fn replay_bytes(&self) -> Option<i64> {
        self.job.input_replay_bytes
    }
    pub fn replay_sha256(&self) -> Option<&str> {
        self.job.input_replay_sha256.as_deref()
    }
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.job.expires_at
    }
}
impl MediaQueue {
    pub async fn claim_paired_candidate(&self) -> Result<Option<PairedCandidateLease>, StoreError> {
        let job: Option<Job> = sqlx::query_as("SELECT * FROM media.claim_paired_candidate()")
            .fetch_optional(&self.pool)
            .await?;
        Ok(job.map(|job| PairedCandidateLease { job }))
    }

    /// Consumes the lease. SQL fences expiry and refuses reuse. Even a checked
    /// result ends in a terminal, non-publishable job without output metadata.
    pub async fn finish_paired_candidate(
        &self,
        lease: PairedCandidateLease,
        checked: bool,
    ) -> Result<(), StoreError> {
        let changed: bool = sqlx::query_scalar("SELECT media.finish_paired_candidate($1,$2,$3)")
            .bind(&lease.job.id)
            .bind(&lease.job.lease_token)
            .bind(checked)
            .fetch_one(&self.pool)
            .await?;
        if !changed {
            return Err(StoreError::Conflict(
                "Paired candidate lease is no longer current.",
            ));
        }
        Ok(())
    }
}
