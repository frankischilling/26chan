//! Complete retained-image reservations; legacy normalization cannot create them.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StillOutput {
    pub format: MediaFormat,
    pub primary: OutputMetadata,
    pub variants: OutputVariants,
    pub mobile: Option<OutputMetadata>,
    pub source: SourceProvenance,
}

impl StillOutput {
    fn validate(&self) -> Result<(), StoreError> {
        fn bounded(
            metadata: &OutputMetadata,
            bytes: i64,
            dimension: i32,
        ) -> Result<(), StoreError> {
            validate_hex(&metadata.sha256, 64)?;
            if !(1..=bytes).contains(&metadata.bytes)
                || !(1..=dimension).contains(&metadata.width)
                || !(1..=dimension).contains(&metadata.height)
            {
                return Err(StoreError::Invalid(
                    "Invalid still image dimensions or bytes.",
                ));
            }
            Ok(())
        }
        let valid_profile = match self.format {
            MediaFormat::Png | MediaFormat::Jpeg => self.source.profile == SourceProfile::StillV6,
            MediaFormat::Gif => {
                self.source.profile == SourceProfile::GifV7 && self.mobile.is_none()
            }
            _ => false,
        };
        if !valid_profile || self.source.retained_bytes != self.primary.bytes {
            return Err(StoreError::Invalid("Invalid retained image provenance."));
        }
        self.source.validate()?;
        bounded(
            &self.primary,
            if self.format == MediaFormat::Gif {
                8_388_608
            } else {
                10_485_760
            },
            10_000,
        )?;
        bounded(&self.variants.thumbnail, 1_048_576, 250)?;
        validate_hex(&self.variants.md5, 32)?;
        let expected_md5: String = self
            .source
            .md5
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if self.variants.md5 != expected_md5
            || self.variants.thumbnail.width > self.primary.width
            || self.variants.thumbnail.height > self.primary.height
        {
            return Err(StoreError::Invalid(
                "Invalid retained image checksum or thumbnail.",
            ));
        }
        if let Some(mobile) = &self.mobile {
            bounded(mobile, 4_194_304, 1024)?;
            if mobile.width.max(mobile.height) != 1024
                || mobile.width > self.primary.width
                || mobile.height > self.primary.height
            {
                return Err(StoreError::Invalid("Invalid mobile image geometry."));
            }
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct StillRecord {
    still_profile: Option<String>,
    thumbnail_format: String,
    mobile_sha256: Option<String>,
    mobile_bytes: Option<i64>,
    mobile_width: Option<i32>,
    mobile_height: Option<i32>,
}

impl StillRecord {
    fn matches(&self, output: &StillOutput) -> bool {
        self.still_profile.as_deref() == Some(output.source.profile.as_str())
            && self.thumbnail_format == "jpeg"
            && self.mobile_sha256.as_deref() == output.mobile.as_ref().map(|m| m.sha256.as_str())
            && self.mobile_bytes == output.mobile.as_ref().map(|m| m.bytes)
            && self.mobile_width == output.mobile.as_ref().map(|m| m.width)
            && self.mobile_height == output.mobile.as_ref().map(|m| m.height)
    }
}

impl MediaQueue {
    /// The caller retains the publication lock through all installs and approval.
    /// A retry must reproduce every retained-file and companion receipt.
    pub async fn prepare_still_output(
        &self,
        job_id: &str,
        token: &str,
        output: &StillOutput,
    ) -> Result<Asset, StoreError> {
        validate_hex(job_id, 32)?;
        validate_hex(token, 32)?;
        output.validate()?;
        let mut tx = self.pool.begin().await?;
        let job: Option<String> = sqlx::query_scalar(
            "SELECT id FROM media.jobs WHERE id=$1 AND input_kind='image-v1' FOR UPDATE",
        )
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await?;
        if job.is_none() {
            return Err(unavailable());
        }
        let reserved: Option<Reservation> = sqlx::query_as("SELECT id,sha256,bytes,width,height,output_format,state FROM media.assets WHERE job_id=$1 AND lease_token=$2 FOR UPDATE")
            .bind(job_id).bind(token).fetch_optional(&mut *tx).await?;
        if let Some(reserved) = &reserved {
            let variants: VariantRecord = sqlx::query_as("SELECT md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height FROM media.assets WHERE id=$1")
                .bind(&reserved.asset.id).fetch_one(&mut *tx).await?;
            let source: SourceRecord = sqlx::query_as("SELECT source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5 FROM media.assets WHERE id=$1")
                .bind(&reserved.asset.id).fetch_one(&mut *tx).await?;
            let still: StillRecord = sqlx::query_as("SELECT still_profile,thumbnail_format,mobile_sha256,mobile_bytes,mobile_width,mobile_height FROM media.assets WHERE id=$1")
                .bind(&reserved.asset.id).fetch_one(&mut *tx).await?;
            if !output.primary.matches(&reserved.asset)
                || output.format != reserved.asset.output_format
                || !variants.matches(Some(&output.variants))
                || !source.matches(Some(&output.source))
                || !still.matches(output)
                || reserved.state == "deleting"
            {
                return Err(unavailable());
            }
            if reserved.state == "approved" {
                tx.commit().await?;
                return Ok(reserved.asset.clone());
            }
        }
        let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media.jobs j JOIN media.image_context c ON c.job_id=j.id WHERE j.id=$1 AND j.input_kind='image-v1' AND j.state='processing' AND j.lease_token=$2 AND j.expires_at>clock_timestamp() AND j.input_bytes=$3)")
            .bind(job_id).bind(token).bind(output.source.input_bytes).fetch_one(&mut *tx).await?;
        if !current {
            return Err(unavailable());
        }
        if let Some(reserved) = reserved {
            tx.commit().await?;
            return Ok(reserved.asset);
        }
        let primary = &output.primary;
        let variants = &output.variants;
        let source = &output.source;
        let mobile = output.mobile.as_ref();
        let asset=sqlx::query_as("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5,output_format,still_profile,thumbnail_format,mobile_sha256,mobile_bytes,mobile_width,mobile_height) VALUES(replace(gen_random_uuid()::text,'-',''),$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$21,$14,$15,$16,$21,'jpeg',$17,$18,$19,$20) RETURNING id,sha256,bytes,width,height,output_format")
            .bind(job_id).bind(token).bind(&primary.sha256).bind(primary.bytes).bind(primary.width).bind(primary.height)
            .bind(&variants.md5).bind(&variants.thumbnail.sha256).bind(variants.thumbnail.bytes).bind(variants.thumbnail.width).bind(variants.thumbnail.height)
            .bind(&source.input_sha256).bind(source.input_bytes).bind(source.retained_bytes).bind(source.md5.as_slice()).bind(output.format)
            .bind(mobile.map(|m|&m.sha256)).bind(mobile.map(|m|m.bytes)).bind(mobile.map(|m|m.width)).bind(mobile.map(|m|m.height)).bind(source.profile.as_str())
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> StillOutput {
        StillOutput {
            format: MediaFormat::Jpeg,
            primary: OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 10_485_760,
                width: 10_000,
                height: 10_000,
            },
            variants: OutputVariants {
                md5: "07".repeat(16),
                thumbnail: OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 1_048_576,
                    width: 250,
                    height: 250,
                },
            },
            mobile: Some(OutputMetadata {
                sha256: "c".repeat(64),
                bytes: 4_194_304,
                width: 1024,
                height: 1024,
            }),
            source: SourceProvenance {
                input_sha256: "d".repeat(64),
                input_bytes: 100,
                profile: SourceProfile::StillV6,
                retained_bytes: 10_485_760,
                md5: [7; 16],
            },
        }
    }

    #[test]
    fn source_gif_requires_its_profile_byte_limit_and_no_mobile_image() {
        let mut gif = output();
        gif.format = MediaFormat::Gif;
        gif.source.profile = SourceProfile::GifV7;
        gif.primary.bytes = 8_388_608;
        gif.source.retained_bytes = gif.primary.bytes;
        gif.mobile = None;
        gif.validate().unwrap();
        for mutate in [
            |o: &mut StillOutput| o.source.profile = SourceProfile::StillV6,
            |o: &mut StillOutput| o.format = MediaFormat::Png,
            |o: &mut StillOutput| o.source.input_bytes = 8_388_609,
            |o: &mut StillOutput| {
                o.primary.bytes = 8_388_609;
                o.source.retained_bytes = 8_388_609;
            },
            |o: &mut StillOutput| o.mobile = output().mobile,
        ] {
            let mut changed = gif.clone();
            mutate(&mut changed);
            assert!(changed.validate().is_err());
        }
    }

    #[test]
    fn complete_still_manifest_has_separate_bounds_and_exact_checksums() {
        let valid = output();
        valid.validate().unwrap();
        let mut png = valid.clone();
        png.format = MediaFormat::Png;
        png.mobile = None;
        png.validate().unwrap();
        for mutate in [
            |o: &mut StillOutput| o.format = MediaFormat::Gif,
            |o: &mut StillOutput| o.source.profile = SourceProfile::PngV1,
            |o: &mut StillOutput| o.source.retained_bytes -= 1,
            |o: &mut StillOutput| o.source.md5[0] = 0,
            |o: &mut StillOutput| o.source.input_bytes = 10_485_761,
            |o: &mut StillOutput| o.primary.width = 10_001,
            |o: &mut StillOutput| o.primary.height = 0,
            |o: &mut StillOutput| o.variants.thumbnail.bytes = 1_048_577,
            |o: &mut StillOutput| o.variants.thumbnail.width = 251,
            |o: &mut StillOutput| o.mobile.as_mut().unwrap().bytes = 4_194_305,
            |o: &mut StillOutput| o.mobile.as_mut().unwrap().width = 1025,
            |o: &mut StillOutput| {
                o.mobile.as_mut().unwrap().width = 512;
                o.mobile.as_mut().unwrap().height = 512;
            },
        ] {
            let mut invalid = valid.clone();
            mutate(&mut invalid);
            assert!(invalid.validate().is_err());
        }
    }
}
