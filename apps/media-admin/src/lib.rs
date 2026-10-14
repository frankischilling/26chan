#![forbid(unsafe_code)]

//! Trusted operator publication. The worker never receives this process's
//! credentials or storage authority. Production media remains disabled.
pub mod backfill;
pub mod drawing;
pub mod paired;
use board_media::{
    ApprovedFiles, InputSnapshot, MAX_INPUT_BYTES, PublicationStore, Quarantine, ValidatedOutput,
    animation::ValidatedAnimation,
    pdf::RetainedPdf,
    source_digest::{
        PngSourceDigestLimits, SourceDigestError, SourceDigestProfile, png_source_processed_digest,
    },
    video::{ValidatedVideo, VideoFormat},
};
use board_media_dispatch::DispatchClient;
use board_store::{
    media::{Failure, MediaQueue},
    media_assets::{
        Asset, MediaFormat, MediaReader, OutputMetadata, OutputVariants, SourceProfile,
        SourceProvenance, VideoMetadata,
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
        let filename = job.filename.to_ascii_lowercase();
        let video = if filename.ends_with(".webm") {
            Some(VideoFormat::Webm)
        } else if filename.ends_with(".mp4") {
            Some(VideoFormat::Mp4)
        } else {
            None
        };
        let provenance = if video.is_some() {
            None
        } else {
            source_provenance(&snapshot).map_err(|_| "private input rejected")?
        };
        // Fixed signature selects an explicit transport. Complex GIF admission
        // and decoding remain in the disposable guest.
        let gif =
            video.is_none() && matches!(snapshot.bytes().get(..6), Some(b"GIF87a" | b"GIF89a"));
        let pdf = video.is_none() && snapshot.bytes().starts_with(b"%PDF-");
        if pdf && !job.filename.to_ascii_lowercase().ends_with(".pdf") {
            return Err("private input rejected");
        }
        let disk = if let Some(format) = video {
            client.process_video(snapshot.bytes(), format.tag()).await
        } else if pdf {
            client.process_pdf(snapshot.bytes(), length).await
        } else if gif {
            client.process_gif(snapshot.bytes(), length).await
        } else {
            client.process(snapshot.bytes(), length).await
        }
        .map_err(|_| "dispatch processing failed")?;
        Ok::<_, &str>((disk, provenance, gif, pdf.then_some(snapshot), video))
    };
    let (disk, provenance, gif, pdf, video) =
        match tokio::time::timeout(std::time::Duration::from_secs(29), processed).await {
            Ok(Ok(disk)) => disk,
            _ => {
                // A stale or unavailable failure update grants no authority and
                // must never replace a newer lease. Retry is an operator decision.
                let _ = queue.fail(&job.id, token, Failure::Processing, false).await;
                return Err("dispatch processing failed".into());
            }
        };
    if let Some(format) = video {
        let video = match ValidatedVideo::read_disk(std::io::Cursor::new(disk), format).await {
            Ok(video) => video,
            Err(_) => {
                let _ = queue
                    .fail(&job.id, token, Failure::InvalidOutput, false)
                    .await;
                return Err("dispatch output rejected".into());
            }
        };
        return publish_video(queue, store, &job.id, token, &video)
            .await
            .map_err(|_| "dispatch approval unavailable".into());
    }
    if gif {
        let animation = match ValidatedAnimation::read_disk(std::io::Cursor::new(disk)).await {
            Ok(animation) => animation,
            Err(_) => {
                let _ = queue
                    .fail(&job.id, token, Failure::InvalidOutput, false)
                    .await;
                return Err("dispatch output rejected".into());
            }
        };
        return publish_animation(queue, store, &job.id, token, &animation)
            .await
            .map_err(|_| "dispatch approval unavailable".into());
    }
    let output = match ValidatedOutput::read_disk(std::io::Cursor::new(disk)).await {
        Ok(output) => output,
        Err(_) => {
            let _ = queue
                .fail(&job.id, token, Failure::InvalidOutput, false)
                .await;
            return Err("dispatch output rejected".into());
        }
    };
    if let Some(snapshot) = pdf {
        let document = RetainedPdf::from_processed_snapshot(snapshot, output)?;
        return publish_pdf(queue, store, &job.id, token, &document)
            .await
            .map_err(|_| "dispatch approval unavailable".into());
    }
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

/// Publish only independently validated frames, with the same lease and
/// uncertain-commit recovery rules as PNG output.
pub async fn publish_animation(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    animation: &ValidatedAnimation,
) -> PublicationResult<Asset> {
    let encoded = animation.encode()?;
    let thumbnail = animation.first_frame()?.thumbnail()?;
    let metadata = OutputMetadata {
        sha256: encoded.sha256().to_owned(),
        bytes: encoded.len() as i64,
        width: encoded.dimensions().0 as i32,
        height: encoded.dimensions().1 as i32,
    };
    let variants = OutputVariants {
        md5: encoded.md5().to_owned(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().to_owned(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let guard = store.try_lock()?;
    let pending = queue
        .prepare_gif_output(job_id, token, &metadata, &variants)
        .await?;
    let receipt = guard.install_gif(pending.id.parse()?, &encoded)?;
    if pending.output_format != MediaFormat::Gif
        || receipt.sha256 != pending.sha256
        || receipt.bytes != pending.bytes as u64
    {
        return Err("output reservation differs from installed bytes".into());
    }
    let receipt = guard.install_thumbnail(pending.id.parse()?, &thumbnail)?;
    if receipt.sha256 != variants.thumbnail.sha256
        || receipt.bytes != variants.thumbnail.bytes as u64
    {
        return Err("thumbnail reservation differs from installed bytes".into());
    }
    Ok(queue.approve_output(job_id, token, &pending.id).await?)
}

/// Publish the checked stopped video envelope under a lease-fenced reservation.
pub async fn publish_video(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    video: &ValidatedVideo,
) -> PublicationResult<Asset> {
    let thumbnail = video.preview().encode()?;
    let metadata = OutputMetadata {
        sha256: video.sha256().to_owned(),
        bytes: video.len() as i64,
        width: video.dimensions().0 as i32,
        height: video.dimensions().1 as i32,
    };
    let variants = OutputVariants {
        md5: video.md5().to_owned(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().to_owned(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let format = match video.format() {
        VideoFormat::Webm => MediaFormat::Webm,
        VideoFormat::Mp4 => MediaFormat::Mp4,
    };
    let video_metadata = VideoMetadata {
        video_duration_micros: video.duration_micros() as i64,
        video_has_audio: video.has_audio(),
    };
    let guard = store.try_lock()?;
    let pending = queue
        .prepare_video_output(job_id, token, &metadata, &variants, format, &video_metadata)
        .await?;
    let receipt = guard.install_video(pending.id.parse()?, video)?;
    if pending.output_format != format
        || receipt.sha256 != pending.sha256
        || receipt.bytes != pending.bytes as u64
    {
        return Err("output reservation differs from installed bytes".into());
    }
    let receipt = guard.install_thumbnail(pending.id.parse()?, &thumbnail)?;
    if receipt.sha256 != variants.thumbnail.sha256
        || receipt.bytes != variants.thumbnail.bytes as u64
    {
        return Err("thumbnail reservation differs from installed bytes".into());
    }
    Ok(queue.approve_output(job_id, token, &pending.id).await?)
}

/// Retain the exact dispatched PDF and publish its separately validated preview.
async fn publish_pdf(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    encoded: &RetainedPdf,
) -> PublicationResult<Asset> {
    let thumbnail = encoded.preview().thumbnail()?;
    let metadata = OutputMetadata {
        sha256: encoded.sha256().to_owned(),
        bytes: encoded.len() as i64,
        width: 1,
        height: 1,
    };
    let variants = OutputVariants {
        md5: encoded.md5().to_owned(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().to_owned(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let guard = store.try_lock()?;
    let pending = queue
        .prepare_pdf_output(job_id, token, &metadata, &variants)
        .await?;
    let receipt = guard.install_pdf(pending.id.parse()?, encoded)?;
    if pending.output_format != MediaFormat::Pdf
        || receipt.sha256 != pending.sha256
        || receipt.bytes != pending.bytes as u64
    {
        return Err("output reservation differs from installed bytes".into());
    }
    let receipt = guard.install_thumbnail(pending.id.parse()?, &thumbnail)?;
    if receipt.sha256 != variants.thumbnail.sha256
        || receipt.bytes != variants.thumbnail.bytes as u64
    {
        return Err("thumbnail reservation differs from installed bytes".into());
    }
    Ok(queue.approve_output(job_id, token, &pending.id).await?)
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
    let id = approved.id.parse()?;
    let size = approved.bytes.try_into()?;
    Ok(match approved.output_format {
        MediaFormat::Png => files.read(id, &approved.sha256, size)?,
        MediaFormat::Jpeg => files.read_still(
            id,
            &approved.sha256,
            size,
            board_media::still::StillFormat::Jpeg,
        )?,
        MediaFormat::Gif => files.read_gif(id, &approved.sha256, size)?,
        MediaFormat::Pdf => files.read_pdf(id, &approved.sha256, size)?,
        MediaFormat::Webm => files.read_video(id, &approved.sha256, size, VideoFormat::Webm)?,
        MediaFormat::Mp4 => files.read_video(id, &approved.sha256, size, VideoFormat::Mp4)?,
    })
}

// Bounded framing only: PNG decoder admission stays inside the isolated guest.
// Unsupported JPEG/GIF/PDF provenance is deliberately unknown, never a raw or
// normalized checksum fallback. Malformed PNG framing fails closed.
fn source_provenance(
    snapshot: &InputSnapshot,
) -> Result<Option<SourceProvenance>, SourceDigestError> {
    // PDF has its own explicit guest transport and never claims PNG cleanup.
    // This signature only selects unknown provenance; it does not approve PDF.
    if snapshot.bytes().starts_with(b"%PDF-") {
        return Ok(None);
    }
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
        for input in [
            b"\xff\xd8jpeg".as_slice(),
            b"GIF87agif",
            b"GIF89agif",
            b"%PDF-1.4\nowned document",
        ] {
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
