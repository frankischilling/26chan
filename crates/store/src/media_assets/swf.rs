//! SWF reservations carry the unchanged input identity and no image derivative.
use super::*;

impl MediaQueue {
    pub async fn prepare_swf_output(
        &self,
        job_id: &str,
        token: &str,
        metadata: &OutputMetadata,
        input: &InputReceipt,
    ) -> Result<Asset, StoreError> {
        validate_hex(job_id, 32)?;
        validate_hex(token, 32)?;
        metadata.validate_format(MediaFormat::Swf)?;
        input.validate()?;
        if metadata.bytes < 8
            || input.component != InputComponent::Upload
            || input.sha256 != metadata.sha256
            || input.bytes != metadata.bytes
        {
            return Err(StoreError::Invalid(
                "SWF output differs from the original input.",
            ));
        }
        let md5: String = input.md5.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        let reserved: Option<Reservation> = sqlx::query_as("SELECT id,sha256,bytes,width,height,output_format,state FROM media.assets WHERE job_id=$1 AND lease_token=$2 FOR UPDATE")
            .bind(job_id).bind(token).fetch_optional(&mut *tx).await?;
        if let Some(reserved) = reserved {
            let stored_md5: Option<String> =
                sqlx::query_scalar("SELECT md5 FROM media.assets WHERE id=$1")
                    .bind(&reserved.asset.id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !metadata.matches(&reserved.asset)
                || reserved.asset.output_format != MediaFormat::Swf
                || stored_md5.as_deref() != Some(&md5)
                || reserved.state == "deleting"
            {
                return Err(unavailable());
            }
            if reserved.state == "approved" {
                input
                    .require_on(&mut tx, job_id, token, &reserved.asset.id)
                    .await?;
                tx.commit().await?;
                return Ok(reserved.asset);
            }
        }
        let asset = sqlx::query_as("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,md5,output_format) SELECT replace(gen_random_uuid()::text,'-',''),id,lease_token,$3,$4,$5,$6,$7,'swf' FROM media.jobs WHERE id=$1 AND input_kind='image-v1' AND state='processing' AND lease_token=$2 AND expires_at>clock_timestamp() AND input_bytes=$4 ON CONFLICT(job_id,lease_token) DO UPDATE SET updated_at=media.assets.updated_at WHERE media.assets.state='pending' AND media.assets.output_format='swf' AND (media.assets.sha256,media.assets.bytes,media.assets.width,media.assets.height,media.assets.md5) IS NOT DISTINCT FROM ($3,$4,$5,$6,$7) RETURNING id,sha256,bytes,width,height,output_format")
            .bind(job_id).bind(token).bind(&metadata.sha256).bind(metadata.bytes).bind(metadata.width).bind(metadata.height).bind(md5)
            .fetch_optional(&mut *tx).await?.ok_or_else(unavailable)?;
        tx.commit().await?;
        Ok(asset)
    }
}
