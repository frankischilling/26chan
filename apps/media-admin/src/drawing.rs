//! Drawing-only paired dispatch and joint image/replay publication.
use crate::PublicationResult;
use board_media::{
    PublicationStore, Quarantine, ValidatedOutput,
    canonical_replay::CanonicalReplay,
    paired::{self, InputKind},
    paired_snapshot::{PairedInputDescriptor, PairedInputSnapshot},
};
use board_media_dispatch::DispatchClient;
use board_store::{
    media::{Failure, MediaQueue},
    media_assets::{
        Asset, DrawingLease, InputComponent, InputReceipt, OutputMetadata, OutputVariants,
        SourceProvenance,
    },
};
use tokio::io::AsyncReadExt;

struct ProcessedDrawing {
    image: ValidatedOutput,
    replay: Option<CanonicalReplay>,
    source: SourceProvenance,
    input: InputReceipt,
}

pub async fn dispatch(
    queue: &MediaQueue,
    quarantine: &Quarantine,
    store: &PublicationStore,
    client: &DispatchClient,
) -> PublicationResult<Asset> {
    let lease = queue
        .claim_drawing()
        .await?
        .ok_or("no queued drawing available")?;
    let processed = tokio::time::timeout(std::time::Duration::from_secs(29), async {
        let descriptor = descriptor(&lease)?;
        let file = quarantine.open_paired_input(descriptor.job_id, descriptor.bytes)?;
        let snapshot = PairedInputSnapshot::read(file, descriptor).await?;
        let source = crate::paired::png_provenance(&snapshot)?;
        let input = InputReceipt {
            component: InputComponent::PairedImage,
            sha256: snapshot
                .descriptor()
                .image_sha256
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes: snapshot.descriptor().image_bytes as i64,
            md5: *snapshot.raw_image_md5(),
        };
        let binding = snapshot.new_attempt_binding()?;
        let disk = client
            .process_paired(snapshot.bytes(), snapshot.bytes().len() as u64, &binding)
            .await?;
        let candidate = paired::decode_result(
            InputKind::PairedV2,
            &disk,
            binding,
            snapshot.replay_presence(),
        )?;
        let mut header = [0_u8; 16];
        header[..8].copy_from_slice(b"IBRGBA01");
        header[8..12].copy_from_slice(&candidate.dimensions().0.to_be_bytes());
        header[12..16].copy_from_slice(&candidate.dimensions().1.to_be_bytes());
        let image = ValidatedOutput::read(header.as_slice().chain(candidate.rgba_bytes())).await?;
        let replay = candidate
            .replay_wire_bytes()
            .map(|bytes| CanonicalReplay::check(bytes.to_vec()))
            .transpose()?;
        if replay.as_ref().is_some_and(|replay| {
            let (width, height) = replay.dimensions();
            (u32::from(width), u32::from(height)) != image.dimensions()
        }) {
            return Err("drawing replay dimensions differ from the image".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(ProcessedDrawing {
            image,
            replay,
            source,
            input,
        })
    })
    .await;
    let processed = match processed {
        Ok(Ok(value)) => value,
        _ => {
            let _ = queue
                .fail_drawing(lease, Failure::InvalidOutput, false)
                .await;
            return Err("drawing input or isolated output rejected".into());
        }
    };
    // Once publication starts, an uncertain approval must remain recoverable.
    // Never fail the lease or remove files after a possibly committed approval.
    publish(queue, store, lease, processed).await
}

async fn publish(
    queue: &MediaQueue,
    store: &PublicationStore,
    lease: DrawingLease,
    drawing: ProcessedDrawing,
) -> PublicationResult<Asset> {
    let image = drawing.image.encode()?;
    let thumbnail = drawing.image.thumbnail()?;
    let metadata = OutputMetadata {
        sha256: image.sha256().into(),
        bytes: image.len() as i64,
        width: image.dimensions().0 as i32,
        height: image.dimensions().1 as i32,
    };
    let variants = OutputVariants {
        md5: image.md5().into(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().into(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let replay_metadata = drawing.replay.as_ref().map(|replay| OutputMetadata {
        sha256: replay.sha256().into(),
        bytes: replay.bytes().len() as i64,
        width: i32::from(replay.dimensions().0),
        height: i32::from(replay.dimensions().1),
    });
    let guard = store.try_lock()?;
    let pending = queue
        .prepare_drawing_output(
            &lease,
            &metadata,
            &variants,
            &drawing.source,
            replay_metadata.as_ref(),
        )
        .await?;
    let token = lease
        .job()
        .lease_token
        .as_deref()
        .ok_or("missing drawing lease")?;
    queue
        .record_input_receipt(&lease.job().id, token, &pending.id, &drawing.input)
        .await?;
    let id = pending.id.parse()?;
    let receipt = guard.install(id, &image)?;
    if receipt.sha256 != pending.sha256 || receipt.bytes != pending.bytes as u64 {
        return Err("drawing image reservation differs".into());
    }
    let receipt = guard.install_thumbnail(id, &thumbnail)?;
    if receipt.sha256 != variants.thumbnail.sha256
        || receipt.bytes != variants.thumbnail.bytes as u64
    {
        return Err("drawing thumbnail reservation differs".into());
    }
    if let Some(replay) = &drawing.replay {
        let receipt = guard.install_replay(id, replay)?;
        if receipt.sha256 != replay.sha256() || receipt.bytes != replay.bytes().len() as u64 {
            return Err("drawing replay reservation differs".into());
        }
    }
    Ok(queue
        .approve_drawing_output_with_input(lease, &pending.id, &drawing.input)
        .await?)
}

fn descriptor(lease: &DrawingLease) -> PublicationResult<PairedInputDescriptor> {
    let job = lease.job();
    let bytes = |value: Option<i64>| -> PublicationResult<u64> {
        Ok(value.ok_or("missing drawing input length")?.try_into()?)
    };
    let replay = match (job.input_replay_bytes, job.input_replay_sha256.as_deref()) {
        (None, None) => None,
        (Some(length), Some(hash)) => Some((length.try_into()?, crate::paired::sha256(hash)?)),
        _ => return Err("incomplete drawing replay descriptor".into()),
    };
    Ok(PairedInputDescriptor {
        job_id: job.id.parse()?,
        bytes: bytes(job.input_bytes)?,
        sha256: crate::paired::sha256(
            job.input_sha256
                .as_deref()
                .ok_or("missing drawing input digest")?,
        )?,
        image_bytes: bytes(job.input_image_bytes)?,
        image_sha256: crate::paired::sha256(
            job.input_image_sha256
                .as_deref()
                .ok_or("missing drawing image digest")?,
        )?,
        replay,
    })
}
