//! Bounded legacy upgrade through the existing authenticated isolated dispatcher.
use crate::PublicationResult;
use board_media::{ApprovedFiles, PublicationGuard, PublicationStore, ValidatedOutput};
use board_media_dispatch::DispatchClient;
use board_store::{
    legacy_media::{LegacyAsset, LegacyMediaStore},
    media_assets::{OutputMetadata, OutputVariants},
};
use std::time::Duration;

pub async fn backfill(
    admin: &LegacyMediaStore,
    store: &PublicationStore,
    files: &ApprovedFiles,
    client: &DispatchClient,
    id: &str,
) -> PublicationResult<bool> {
    let guard = store.try_lock()?;
    let before = admin.get(id).await?;
    let full = files.read(
        id.parse()?,
        &before.asset.sha256,
        before.asset.bytes.try_into()?,
    )?;
    if let Some(variants) = before.variants()? {
        files.read_thumbnail(
            id.parse()?,
            &variants.thumbnail.sha256,
            variants.thumbnail.bytes.try_into()?,
        )?;
        return Ok(false);
    }
    let length = full.len() as u64;
    let disk = tokio::time::timeout(
        Duration::from_secs(29),
        client.process(std::io::Cursor::new(full), length),
    )
    .await??;
    let output = ValidatedOutput::read_disk(std::io::Cursor::new(disk)).await?;
    complete_backfill(admin, &guard, files, &before, &output).await?;
    Ok(true)
}

/// Worker pixels remain untrusted. Re-encoding must reproduce the original
/// approved bytes before any thumbnail install or database mutation is allowed.
pub async fn complete_backfill(
    admin: &LegacyMediaStore,
    guard: &PublicationGuard<'_>,
    files: &ApprovedFiles,
    before: &LegacyAsset,
    output: &ValidatedOutput,
) -> PublicationResult<()> {
    if before.variants()?.is_some() {
        return Err("manifest is already populated".into());
    }
    let full = output.encode()?;
    let expected = &before.asset;
    if full.sha256() != expected.sha256
        || full.len() != expected.bytes as u64
        || full.dimensions() != (expected.width as u32, expected.height as u32)
    {
        return Err("isolated output does not reproduce the approved file".into());
    }
    files.read(expected.id.parse()?, full.sha256(), full.len())?;
    let thumbnail = output.thumbnail()?;
    let variants = OutputVariants {
        md5: full.md5().to_owned(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().to_owned(),
            bytes: thumbnail.len() as i64,
            width: thumbnail.dimensions().0 as i32,
            height: thumbnail.dimensions().1 as i32,
        },
    };
    let receipt = guard.install_thumbnail(expected.id.parse()?, &thumbnail)?;
    if receipt.sha256 != variants.thumbnail.sha256 || receipt.bytes != thumbnail.len() {
        return Err("installed thumbnail differs from manifest".into());
    }
    admin.commit(expected, &variants).await?;
    Ok(())
}
