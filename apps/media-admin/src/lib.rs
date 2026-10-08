#![forbid(unsafe_code)]

//! Trusted operator publication. The worker never receives this process's
//! credentials or storage authority. Production media remains disabled.
pub mod backfill;
pub mod paired;
use board_media::{
    ApprovedFiles, InputSnapshot, MAX_INPUT_BYTES, PublicationStore, Quarantine, ValidatedOutput,
    source_digest::{
        PngSourceDigestLimits, SourceDigestError, SourceDigestProfile, png_source_processed_digest,
    },
};
use board_media_dispatch::DispatchClient;
use board_store::{
    media::{Failure, MediaQueue},
    media_assets::{
        Asset, MediaReader, OutputMetadata, OutputVariants, SourceProfile, SourceProvenance,
    },
};

pub type PublicationResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Dispatch one job after the caller initializes every local root and TLS
/// configuration. Transport success is untrusted until validation and fenced
/// durable approval. There is no automatic retry or raw-output fallback.
pub async fn dispatch(
    queue: &MediaQueue,
    quarantine: &Quarantine,
    store: &PublicationStore,
    client: &DispatchClient,
) -> PublicationResult<Asset> {
    let job = queue
        .claim()
        .await
        .map_err(|_| "queue claim unavailable")?
        .ok_or("no queued job available")?;
    let token = job.lease_token.as_deref().ok_or("missing lease")?;
    let processed = async {
        let length: u64 = job
            .input_bytes
            .ok_or("missing input length")?
            .try_into()
            .map_err(|_| "invalid input length")?;
        let input = quarantine
            .open_input(job.id.parse().map_err(|_| "invalid job ID")?, length)
            .map_err(|_| "private input rejected")?;
        let snapshot = InputSnapshot::read(input, length)
            .await
            .map_err(|_| "private input rejected")?;
        let provenance = source_provenance(&snapshot).map_err(|_| "private input rejected")?;
        let disk = client
            .process(snapshot.bytes(), length)
            .await
            .map_err(|_| "dispatch processing failed")?;
        Ok::<_, &str>((disk, provenance))
    };
    let (disk, provenance) =
        match tokio::time::timeout(std::time::Duration::from_secs(29), processed).await {
            Ok(Ok(disk)) => disk,
            _ => {
                // A stale or unavailable failure update grants no authority and
                // must never replace a newer lease. Retry is an operator decision.
                let _ = queue.fail(&job.id, token, Failure::Processing, false).await;
                return Err("dispatch processing failed".into());
            }
        };
    let output = match ValidatedOutput::read_disk(std::io::Cursor::new(disk)).await {
        Ok(output) => output,
        Err(_) => {
            let _ = queue
                .fail(&job.id, token, Failure::InvalidOutput, false)
                .await;
            return Err("dispatch output rejected".into());
        }
    };
    // Preserve publication's reservation/lock and uncertain-commit recovery.
    // Never remove or fail a possibly approved object after an approval error.
    publish_with_provenance(queue, store, &job.id, token, &output, provenance.as_ref())
        .await
        .map_err(|_| "dispatch approval unavailable".into())
}

pub async fn publish(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    output: &ValidatedOutput,
) -> PublicationResult<Asset> {
    publish_with_provenance(queue, store, job_id, token, output, None).await
}

// Only the coordinator may attach source provenance, after guest output passed
// validation. Manual publication and normalized-output backfill remain unknown.
async fn publish_with_provenance(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    output: &ValidatedOutput,
    provenance: Option<&SourceProvenance>,
) -> PublicationResult<Asset> {
    let encoded = output.encode()?;
    let thumbnail = output.thumbnail()?;
    let metadata = OutputMetadata {
        sha256: encoded.sha256().to_owned(),
        bytes: encoded.len() as i64,
        width: encoded.dimensions().0 as i32,
        height: encoded.dimensions().1 as i32,
    };
    let guard = store.try_lock()?;
    let variants = OutputVariants {
        md5: encoded.md5().to_owned(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().to_owned(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let pending = queue
        .prepare_output_with_provenance(job_id, token, &metadata, Some(&variants), provenance)
        .await?;
    let receipt = guard.install(pending.id.parse()?, &encoded)?;
    if receipt.sha256 != pending.sha256 || receipt.bytes != pending.bytes as u64 {
        return Err("output reservation differs from installed bytes".into());
    }
    let thumbnail_receipt = guard.install_thumbnail(pending.id.parse()?, &thumbnail)?;
    if thumbnail_receipt.sha256 != variants.thumbnail.sha256
        || thumbnail_receipt.bytes != variants.thumbnail.bytes as u64
    {
        return Err("thumbnail reservation differs from installed bytes".into());
    }
    // Failure/uncertain commit leaves a reserved object for retry or cleanup;
    // no error path removes a possibly approved file.
    Ok(queue.approve_output(job_id, token, &pending.id).await?)
}

pub async fn reconcile(queue: &MediaQueue, store: &PublicationStore) -> PublicationResult<u64> {
    let guard = store.try_lock()?;
    for id in queue.output_retention_candidates().await? {
        queue.retire_output(&id).await?;
    }
    let mut removed = 0;
    for id in queue.output_cleanup_candidates().await? {
        if queue.begin_output_deletion(&id).await? {
            guard.remove(id.parse()?)?;
            if queue.forget_output(&id).await? {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

pub async fn read_approved(
    reader: &MediaReader,
    files: &ApprovedFiles,
    id: &str,
) -> PublicationResult<Vec<u8>> {
    let approved = reader.get(id).await?;
    Ok(files.read(
        approved.id.parse()?,
        &approved.sha256,
        approved.bytes.try_into()?,
    )?)
}

// Bounded framing only: PNG decoder admission stays inside the isolated guest.
// Unsupported JPEG/GIF provenance is deliberately unknown, never a raw or
// normalized checksum fallback. Malformed PNG framing fails closed.
fn source_provenance(
    snapshot: &InputSnapshot,
) -> Result<Option<SourceProvenance>, SourceDigestError> {
    let limit = MAX_INPUT_BYTES as usize;
    let limits = PngSourceDigestLimits::new(limit, limit, limit)?;
    match png_source_processed_digest(snapshot.bytes(), limits) {
        Ok(digest) => Ok(Some(SourceProvenance {
            input_sha256: snapshot
                .raw_sha256()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            input_bytes: snapshot.bytes().len() as i64,
            profile: match digest.profile() {
                SourceDigestProfile::PngV1 => SourceProfile::PngV1,
            },
            retained_bytes: digest.processed_bytes() as i64,
            md5: *digest.md5(),
        })),
        Err(SourceDigestError::JpegUnavailable | SourceDigestError::GifUnavailable) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    async fn snapshot(bytes: &[u8]) -> InputSnapshot {
        let temp = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(temp.path()).unwrap();
        let id = board_media::ObjectId::generate().unwrap();
        let length = quarantine.receive(id, bytes).await.unwrap();
        InputSnapshot::read(quarantine.open_input(id, length).unwrap(), length)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn coordinator_provenance_has_no_unsupported_or_malformed_hash_fallback() {
        for input in [b"\xff\xd8jpeg".as_slice(), b"GIF87agif", b"GIF89agif"] {
            assert_eq!(source_provenance(&snapshot(input).await).unwrap(), None);
        }
        for input in [
            b"unknown".as_slice(),
            b"\x89PNG\r\n\x1a\n",
            b"\x89PNG\r\n\x1a\n\0\0",
        ] {
            assert!(source_provenance(&snapshot(input).await).is_err());
        }
    }
}
