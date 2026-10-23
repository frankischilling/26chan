//! Context-bound still dispatch; parsing stays inside the disposable worker.
use super::*;
use board_media::still::{EncodedStillFile, StillFormat, ValidatedStill, source_policy};
use board_store::{
    media::{ImageContext, Job},
    media_assets::StillOutput,
};

pub(crate) fn format(input: &[u8]) -> Option<StillFormat> {
    if input.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(StillFormat::Png)
    } else if input.starts_with(b"\xff\xd8\xff") {
        Some(StillFormat::Jpeg)
    } else if input.starts_with(b"GIF87a") || input.starts_with(b"GIF89a") {
        Some(StillFormat::Gif)
    } else {
        None
    }
}

pub(crate) async fn dispatch_context(
    queue: &MediaQueue,
    store: &PublicationStore,
    client: &DispatchClient,
    job: &Job,
    context: &ImageContext,
    snapshot: InputSnapshot,
) -> PublicationResult<Asset> {
    let token = job.lease_token.as_deref().ok_or("missing lease")?;
    let format = format(snapshot.bytes()).ok_or("unsupported still input")?;
    let policy = source_policy(
        &context.board,
        format,
        context.thread_id == 0,
        context.capcode,
    )
    .ok_or("unknown image context")?;
    queue.extend_still_lease(&job.id, token).await?;
    let work = async {
        let disk = if format == StillFormat::Gif {
            client
                .process_source_gif(snapshot.bytes(), &policy.header()?)
                .await?
        } else {
            client
                .process_still(snapshot.bytes(), &policy.header()?)
                .await?
        };
        let output = ValidatedStill::read_disk(std::io::Cursor::new(disk), policy).await?;
        publish(queue, store, &job.id, token, &snapshot, &output).await
    };
    match tokio::time::timeout(std::time::Duration::from_secs(80), work).await {
        Ok(result) => result,
        Err(_) => Err("still processing deadline exceeded".into()),
    }
}

fn metadata(file: &EncodedStillFile) -> OutputMetadata {
    let (width, height) = file.dimensions();
    OutputMetadata {
        sha256: file.sha256().into(),
        bytes: file.bytes().len() as i64,
        width: width as i32,
        height: height as i32,
    }
}

async fn publish(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    snapshot: &InputSnapshot,
    output: &ValidatedStill,
) -> PublicationResult<Asset> {
    let mut md5 = [0; 16];
    for (index, byte) in md5.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&output.retained().md5()[index * 2..index * 2 + 2], 16)?;
    }
    let manifest = StillOutput {
        exif: output
            .exif()
            .map(|text| board_store::media_assets::ExifMetadata {
                text: text.to_owned(),
                mode: i16::from(output.policy().exif_mode),
            }),
        format: match output.policy().format {
            StillFormat::Png => MediaFormat::Png,
            StillFormat::Jpeg => MediaFormat::Jpeg,
            StillFormat::Gif => MediaFormat::Gif,
        },
        primary: metadata(output.retained()),
        variants: OutputVariants {
            md5: output.retained().md5().into(),
            thumbnail: metadata(output.thumbnail()),
        },
        mobile: output.mobile().map(metadata),
        source: SourceProvenance {
            input_sha256: snapshot
                .raw_sha256()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            input_bytes: snapshot.bytes().len() as i64,
            profile: if output.policy().format == StillFormat::Gif {
                SourceProfile::GifV7
            } else {
                SourceProfile::StillV6
            },
            retained_bytes: output.retained().bytes().len() as i64,
            md5,
        },
    };
    let guard = store.try_lock()?;
    let asset = queue.prepare_still_output(job_id, token, &manifest).await?;
    queue
        .record_input_receipt(job_id, token, &asset.id, &input_receipt(snapshot))
        .await?;
    let id = asset.id.parse()?;
    let primary = guard.install_still(id, output)?;
    if primary.sha256 != asset.sha256
        || primary.bytes != asset.bytes as u64
        || asset.output_format != manifest.format
    {
        return Err("retained image receipt mismatch".into());
    }
    let thumbnail = guard.install_still_thumbnail(id, output)?;
    if thumbnail.sha256 != manifest.variants.thumbnail.sha256
        || thumbnail.bytes != manifest.variants.thumbnail.bytes as u64
    {
        return Err("image thumbnail receipt mismatch".into());
    }
    let mobile = guard.install_still_mobile(id, output)?;
    match (mobile, manifest.mobile.as_ref()) {
        (None, None) => {}
        (Some(receipt), Some(expected))
            if receipt.sha256 == expected.sha256 && receipt.bytes == expected.bytes as u64 => {}
        _ => return Err("mobile image receipt mismatch".into()),
    }
    Ok(queue
        .approve_output_with_input(job_id, token, &asset.id, &input_receipt(snapshot))
        .await?)
}
