//! Durable approval authority and the restricted approval reader.
//!
//! Callers must hold their publication storage lock throughout every reservation,
//! approval, and output-cleanup operation, including the associated file changes.
use crate::{
    StoreError,
    media::{MediaQueue, changed, validate_hex},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Asset {
    pub id: String,
    pub sha256: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputMetadata {
    pub sha256: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputVariants {
    pub md5: String,
    pub thumbnail: OutputMetadata,
}

#[derive(sqlx::FromRow)]
struct VariantRecord {
    md5: Option<String>,
    thumbnail_sha256: Option<String>,
    thumbnail_bytes: Option<i64>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
}

impl VariantRecord {
    fn matches(&self, variants: Option<&OutputVariants>) -> bool {
        self.md5.as_deref() == variants.map(|v| v.md5.as_str())
            && self.thumbnail_sha256.as_deref() == variants.map(|v| v.thumbnail.sha256.as_str())
            && self.thumbnail_bytes == variants.map(|v| v.thumbnail.bytes)
            && self.thumbnail_width == variants.map(|v| v.thumbnail.width)
            && self.thumbnail_height == variants.map(|v| v.thumbnail.height)
    }
}

impl OutputMetadata {
    fn validate(&self) -> Result<(), StoreError> {
        validate_hex(&self.sha256, 64)?;
        if !(1..=5_242_880).contains(&self.bytes)
            || !(1..=1024).contains(&self.width)
            || !(1..=1024).contains(&self.height)
        {
            return Err(StoreError::Invalid(
                "Output metadata is outside the permitted range.",
            ));
        }
        Ok(())
    }

    fn matches(&self, asset: &Asset) -> bool {
        self.sha256 == asset.sha256
            && self.bytes == asset.bytes
            && self.width == asset.width
            && self.height == asset.height
    }
}

#[derive(sqlx::FromRow)]
struct Reservation {
    #[sqlx(flatten)]
    asset: Asset,
    state: String,
}

fn unavailable() -> StoreError {
    StoreError::Conflict("Media reservation or lease is no longer valid.")
}

impl MediaQueue {
    /// Reserve exact host-encoded bytes while holding the publication storage lock.
    pub async fn prepare_output(
        &self,
        job_id: &str,
        token: &str,
        metadata: &OutputMetadata,
    ) -> Result<Asset, StoreError> {
        self.prepare_output_with_variants(job_id, token, metadata, None)
            .await
    }

    pub async fn prepare_output_with_variants(
        &self,
        job_id: &str,
        token: &str,
        metadata: &OutputMetadata,
        variants: Option<&OutputVariants>,
    ) -> Result<Asset, StoreError> {
        validate_hex(job_id, 32)?;
        validate_hex(token, 32)?;
        metadata.validate()?;
        if let Some(variants) = variants {
            validate_hex(&variants.md5, 32)?;
            variants.thumbnail.validate()?;
            if variants.thumbnail.width > 250
                || variants.thumbnail.height > 250
                || variants.thumbnail.width > metadata.width
                || variants.thumbnail.height > metadata.height
            {
                return Err(StoreError::Invalid("Invalid thumbnail dimensions."));
            }
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM media.jobs WHERE id = $1 FOR UPDATE")
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await?;
        let reserved: Option<Reservation> = sqlx::query_as("SELECT id, sha256, bytes, width, height, state FROM media.assets WHERE job_id = $1 AND lease_token = $2 FOR UPDATE")
            .bind(job_id).bind(token).fetch_optional(&mut *tx).await?;
        if let Some(reserved) = reserved {
            let stored: VariantRecord = sqlx::query_as("SELECT md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height FROM media.assets WHERE id=$1")
                .bind(&reserved.asset.id).fetch_one(&mut *tx).await?;
            if !metadata.matches(&reserved.asset)
                || !stored.matches(variants)
                || reserved.state == "deleting"
            {
                return Err(unavailable());
            }
            if reserved.state == "approved" {
                tx.commit().await?;
                return Ok(reserved.asset);
            }
        }
        // The statement that inserts or reuses pending metadata rechecks the lease.
        let asset = sqlx::query_as("INSERT INTO media.assets (id, job_id, lease_token, sha256, bytes, width, height,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height) SELECT replace(gen_random_uuid()::text, '-', ''), id, lease_token, $3, $4, $5, $6,$7,$8,$9,$10,$11 FROM media.jobs WHERE id = $1 AND state = 'processing' AND lease_token = $2 AND expires_at > clock_timestamp() ON CONFLICT (job_id, lease_token) DO UPDATE SET updated_at = media.assets.updated_at WHERE media.assets.state = 'pending' AND media.assets.sha256 = $3 AND media.assets.bytes = $4 AND media.assets.width = $5 AND media.assets.height = $6 AND (media.assets.md5,media.assets.thumbnail_sha256,media.assets.thumbnail_bytes,media.assets.thumbnail_width,media.assets.thumbnail_height) IS NOT DISTINCT FROM ($7,$8,$9,$10,$11) AND EXISTS (SELECT 1 FROM media.jobs WHERE id = $1 AND state = 'processing' AND lease_token = $2 AND expires_at > clock_timestamp()) RETURNING id, sha256, bytes, width, height")
            .bind(job_id).bind(token).bind(&metadata.sha256).bind(metadata.bytes).bind(metadata.width).bind(metadata.height)
            .bind(variants.map(|v| &v.md5)).bind(variants.map(|v| &v.thumbnail.sha256))
            .bind(variants.map(|v| v.thumbnail.bytes)).bind(variants.map(|v| v.thumbnail.width)).bind(variants.map(|v| v.thumbnail.height))
            .fetch_optional(&mut *tx).await?.ok_or_else(unavailable)?;
        tx.commit().await?;
        Ok(asset)
    }

    /// Approve installed bytes and complete the current lease in one transaction.
    /// The caller must retain the publication storage lock until this returns.
    pub async fn approve_output(
        &self,
        job_id: &str,
        token: &str,
        output_id: &str,
    ) -> Result<Asset, StoreError> {
        validate_hex(job_id, 32)?;
        validate_hex(token, 32)?;
        validate_hex(output_id, 32)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM media.jobs WHERE id = $1 FOR UPDATE")
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await?;
        let reserved: Reservation = sqlx::query_as("SELECT id, sha256, bytes, width, height, state FROM media.assets WHERE id = $3 AND job_id = $1 AND lease_token = $2 FOR UPDATE")
            .bind(job_id).bind(token).bind(output_id).fetch_optional(&mut *tx).await?.ok_or_else(unavailable)?;
        if reserved.state == "approved" {
            tx.commit().await?;
            return Ok(reserved.asset);
        }
        if reserved.state != "pending" {
            return Err(unavailable());
        }
        changed(sqlx::query("UPDATE media.jobs SET state = 'published', output_sha256 = $3, output_bytes = $4, expires_at = NULL, updated_at = clock_timestamp() WHERE id = $1 AND state = 'processing' AND lease_token = $2 AND expires_at > clock_timestamp()")
            .bind(job_id).bind(token).bind(&reserved.asset.sha256).bind(reserved.asset.bytes)
            .execute(&mut *tx).await?.rows_affected())?;
        changed(sqlx::query("UPDATE media.assets SET state = 'approved', approved_at = clock_timestamp(), updated_at = clock_timestamp() WHERE id = $1 AND state = 'pending'")
            .bind(output_id).execute(&mut *tx).await?.rows_affected())?;
        tx.commit().await?;
        Ok(reserved.asset)
    }

    /// Select at most 64 abandoned reservations under the publication storage lock.
    pub async fn output_cleanup_candidates(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar("SELECT a.id FROM media.assets a WHERE a.state = 'deleting' OR (a.state = 'pending' AND NOT EXISTS (SELECT 1 FROM media.jobs j WHERE j.id = a.job_id AND j.state = 'processing' AND j.lease_token = a.lease_token AND j.expires_at > clock_timestamp())) ORDER BY a.created_at, a.id LIMIT 64")
            .fetch_all(&self.pool).await?)
    }

    /// IDs only: the coordinator cannot read the attachment or capability tables.
    pub async fn output_retention_candidates(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM media.retirable_outputs ORDER BY approved_at,id LIMIT 64",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// Recheck visibility/retention under the posting job lock. Retain the
    /// publication storage lock through retirement, removal and forget_output.
    pub async fn retire_output(&self, id: &str) -> Result<bool, StoreError> {
        validate_hex(id, 32)?;
        Ok(sqlx::query_scalar("SELECT media.retire_output($1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?)
    }

    /// Recheck abandonment before removing files; retain the storage lock through forget_output.
    pub async fn begin_output_deletion(&self, output_id: &str) -> Result<bool, StoreError> {
        validate_hex(output_id, 32)?;
        let mut tx = self.pool.begin().await?;
        let job_id: Option<String> =
            sqlx::query_scalar("SELECT job_id FROM media.assets WHERE id = $1")
                .bind(output_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(job_id) = job_id else {
            return Ok(false);
        };
        sqlx::query("SELECT id FROM media.jobs WHERE id = $1 FOR UPDATE")
            .bind(&job_id)
            .fetch_optional(&mut *tx)
            .await?;
        sqlx::query("SELECT id FROM media.assets WHERE id = $1 FOR UPDATE")
            .bind(output_id)
            .fetch_optional(&mut *tx)
            .await?;
        let deleted = sqlx::query("UPDATE media.assets a SET state = 'deleting', updated_at = clock_timestamp() WHERE a.id = $1 AND (a.state = 'deleting' OR (a.state = 'pending' AND NOT EXISTS (SELECT 1 FROM media.jobs j WHERE j.id = a.job_id AND j.state = 'processing' AND j.lease_token = a.lease_token AND j.expires_at > clock_timestamp())))")
            .bind(output_id).execute(&mut *tx).await?.rows_affected() == 1;
        tx.commit().await?;
        Ok(deleted)
    }

    /// Remove deleting metadata only after file removal and sync, with the storage lock held.
    pub async fn forget_output(&self, output_id: &str) -> Result<bool, StoreError> {
        validate_hex(output_id, 32)?;
        Ok(
            sqlx::query("DELETE FROM media.assets WHERE id = $1 AND state = 'deleting'")
                .bind(output_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1,
        )
    }
}

#[derive(Clone)]
pub struct MediaReader {
    pool: PgPool,
}

impl MediaReader {
    /// Process-local pool occupancy only; does not query or expose the database.
    pub fn pool_statistics(&self) -> (u32, usize, u32) {
        (
            self.pool.size(),
            self.pool.num_idle(),
            self.pool.options().get_max_connections(),
        )
    }

    pub async fn ready(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT id FROM media.approved_assets LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        sqlx::query("SELECT id FROM media.approved_post_assets LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(3))
            .connect(url)
            .await?;
        let safe: bool = sqlx::query_scalar(
            "SELECT current_user = 'board_media_read'
             AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls
             AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member = pg_roles.oid)
             AND NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = current_database() AND datdba = pg_roles.oid)
             AND NOT has_database_privilege(current_user, current_database(), 'CREATE')
             AND NOT has_schema_privilege(current_user, 'media', 'CREATE')
             AND NOT has_schema_privilege(current_user, 'content', 'USAGE')
             AND NOT has_schema_privilege(current_user, 'post_secrets', 'USAGE')
             AND NOT has_schema_privilege(current_user, 'staff_identity', 'USAGE')
             AND NOT has_schema_privilege(current_user, 'deployment', 'USAGE')
             AND has_schema_privilege(current_user, 'media', 'USAGE')
             AND NOT has_table_privilege(current_user, 'media.jobs', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
             AND NOT has_any_column_privilege(current_user, 'media.jobs', 'SELECT,INSERT,UPDATE,REFERENCES')
             AND NOT has_table_privilege(current_user, 'media.assets', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
             AND NOT has_any_column_privilege(current_user, 'media.assets', 'SELECT,INSERT,UPDATE,REFERENCES')
             AND NOT has_table_privilege(current_user, 'media.queue_policy', 'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
             AND NOT has_any_column_privilege(current_user, 'media.queue_policy', 'SELECT,INSERT,UPDATE,REFERENCES')
             AND NOT has_table_privilege(current_user, 'media.approved_assets', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
             AND NOT has_any_column_privilege(current_user, 'media.approved_assets', 'INSERT,UPDATE,REFERENCES')
             AND has_table_privilege(current_user, 'media.approved_assets', 'SELECT')
             AND NOT has_table_privilege(current_user, 'media.approved_post_assets', 'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
             AND NOT has_any_column_privilege(current_user, 'media.approved_post_assets', 'INSERT,UPDATE,REFERENCES')
             AND has_table_privilege(current_user, 'media.approved_post_assets', 'SELECT')
             FROM pg_roles WHERE rolname = current_user")
            .fetch_one(&pool).await?;
        if !safe {
            pool.close().await;
            return Err(StoreError::UnsafeRole);
        }
        Ok(Self { pool })
    }

    pub async fn get(&self, output_id: &str) -> Result<Asset, StoreError> {
        validate_hex(output_id, 32)?;
        sqlx::query_as(
            "SELECT id, sha256, bytes, width, height FROM media.approved_assets WHERE id = $1",
        )
        .bind(output_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::NotFound)
    }

    pub async fn get_thumbnail(&self, output_id: &str) -> Result<Asset, StoreError> {
        validate_hex(output_id, 32)?;
        sqlx::query_as("SELECT id,thumbnail_sha256 AS sha256,thumbnail_bytes AS bytes,thumbnail_width AS width,thumbnail_height AS height FROM media.approved_assets WHERE id=$1 AND thumbnail_sha256 IS NOT NULL")
            .bind(output_id).fetch_optional(&self.pool).await?.ok_or(StoreError::NotFound)
    }

    pub async fn get_post(
        &self,
        board: &str,
        tim: i64,
        thumbnail: bool,
    ) -> Result<Asset, StoreError> {
        board_domain::BoardSlug::parse(board).map_err(|_| StoreError::NotFound)?;
        if !(1..=9_007_199_254_740_991).contains(&tim) {
            return Err(StoreError::NotFound);
        }
        let statement = if thumbnail {
            "SELECT id,thumbnail_sha256 AS sha256,thumbnail_bytes AS bytes,thumbnail_width AS width,thumbnail_height AS height FROM media.approved_post_assets WHERE board=$1 AND tim=$2 AND thumbnail_sha256 IS NOT NULL"
        } else {
            "SELECT id,sha256,bytes,width,height FROM media.approved_post_assets WHERE board=$1 AND tim=$2"
        };
        sqlx::query_as(statement)
            .bind(board)
            .bind(tim)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::NotFound)
    }
}
