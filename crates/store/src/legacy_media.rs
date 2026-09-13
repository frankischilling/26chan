//! Offline migration authority only. Never construct this store in a runtime.
use crate::{
    StoreError,
    media::validate_hex,
    media_assets::{Asset, OutputMetadata, OutputVariants},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

pub struct LegacyMediaStore {
    pool: PgPool,
}

#[derive(sqlx::FromRow)]
pub struct LegacyAsset {
    #[sqlx(flatten)]
    pub asset: Asset,
    md5: Option<String>,
    thumbnail_sha256: Option<String>,
    thumbnail_bytes: Option<i64>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
}

impl LegacyAsset {
    pub fn variants(&self) -> Result<Option<OutputVariants>, StoreError> {
        match (
            &self.md5,
            &self.thumbnail_sha256,
            self.thumbnail_bytes,
            self.thumbnail_width,
            self.thumbnail_height,
        ) {
            (None, None, None, None, None) => Ok(None),
            (Some(md5), Some(sha256), Some(bytes), Some(width), Some(height)) => {
                Ok(Some(OutputVariants {
                    md5: md5.clone(),
                    thumbnail: OutputMetadata {
                        sha256: sha256.clone(),
                        bytes,
                        width,
                        height,
                    },
                }))
            }
            _ => Err(StoreError::Invalid("Incomplete normalized manifest.")),
        }
    }
}

impl LegacyMediaStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(2))
            .connect(url)
            .await?;
        let identity: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await?;
        if identity != "board_migrator" {
            return Err(StoreError::UnsafeRole);
        }
        Ok(Self { pool })
    }

    /// Caller holds the canonical publication lock before reading this snapshot.
    pub async fn get(&self, id: &str) -> Result<LegacyAsset, StoreError> {
        validate_hex(id, 32)?;
        sqlx::query_as("SELECT a.id,a.sha256,a.bytes,a.width,a.height,a.md5,a.thumbnail_sha256,a.thumbnail_bytes,a.thumbnail_width,a.thumbnail_height FROM media.assets a JOIN media.approved_assets v ON v.id=a.id WHERE a.id=$1")
            .bind(id).fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)
    }

    /// Install/fsync the exact thumbnail before this call. A failed or uncertain
    /// commit leaves it private while the database manifest is NULL. Retry under
    /// the same publication lock; never remove a possibly committed thumbnail.
    pub async fn commit(
        &self,
        expected: &Asset,
        variants: &OutputVariants,
    ) -> Result<(), StoreError> {
        validate_hex(&expected.id, 32)?;
        // A public upload can become attached while processing runs. Its link
        // is one-use and durable, so at most one restart is needed to discover
        // the board and retain board -> thread -> job -> asset lock ordering.
        for _ in 0..2 {
            if self.commit_once(expected, variants).await? {
                return Ok(());
            }
        }
        Err(StoreError::Conflict(
            "Attachment changed during legacy upgrade.",
        ))
    }

    async fn commit_once(
        &self,
        expected: &Asset,
        variants: &OutputVariants,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SET LOCAL lock_timeout='2s'")
            .execute(&mut *tx)
            .await?;
        // Match public/staff lock order. Metadata changes invalidate HTTP date
        // validators as well as body ETags, without changing the post or media ID.
        let attached: Option<(String,i64)> = sqlx::query_as("SELECT p.board,p.thread_id FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE m.asset_id=$1")
            .bind(&expected.id).fetch_optional(&mut *tx).await?;
        if let Some((board, thread)) = &attached {
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(board)
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("SELECT id FROM content.threads WHERE board=$1 AND id=$2 FOR UPDATE")
                .bind(board)
                .bind(thread)
                .fetch_one(&mut *tx)
                .await?;
        }
        sqlx::query("SELECT j.id FROM media.jobs j JOIN media.assets a ON a.job_id=j.id WHERE a.id=$1 FOR UPDATE OF j")
            .bind(&expected.id).fetch_optional(&mut *tx).await?;
        if attached.is_none() {
            let now_attached: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM content.post_media WHERE asset_id=$1)",
            )
            .bind(&expected.id)
            .fetch_one(&mut *tx)
            .await?;
            if now_attached {
                // Never wait for a board while holding the job lock: a posting
                // transaction may hold that board and be waiting for this job.
                tx.rollback().await?;
                return Ok(false);
            }
        }
        let changed = sqlx::query("UPDATE media.assets SET md5=$6,thumbnail_sha256=$7,thumbnail_bytes=$8,thumbnail_width=$9,thumbnail_height=$10,updated_at=clock_timestamp() WHERE id=$1 AND sha256=$2 AND bytes=$3 AND width=$4 AND height=$5 AND state='approved' AND md5 IS NULL AND EXISTS (SELECT 1 FROM media.approved_assets v WHERE v.id=$1)")
            .bind(&expected.id).bind(&expected.sha256).bind(expected.bytes).bind(expected.width).bind(expected.height)
            .bind(&variants.md5).bind(&variants.thumbnail.sha256).bind(variants.thumbnail.bytes).bind(variants.thumbnail.width).bind(variants.thumbnail.height)
            .execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(StoreError::Conflict(
                "Legacy approval changed or is unavailable.",
            ));
        }
        if let Some((board, thread)) = attached {
            sqlx::query(
                "UPDATE content.threads SET modified_at=clock_timestamp() WHERE board=$1 AND id=$2",
            )
            .bind(board)
            .bind(thread)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}
