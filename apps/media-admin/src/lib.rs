#![forbid(unsafe_code)]

//! Trusted operator publication. The worker never receives this process's
//! credentials or storage authority. Public media remains disabled.
use board_media::{ApprovedFiles, PublicationStore, ValidatedOutput};
use board_store::{
    media::MediaQueue,
    media_assets::{Asset, MediaReader, OutputMetadata},
};

pub type PublicationResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub async fn publish(
    queue: &MediaQueue,
    store: &PublicationStore,
    job_id: &str,
    token: &str,
    output: &ValidatedOutput,
) -> PublicationResult<Asset> {
    let encoded = output.encode()?;
    let metadata = OutputMetadata {
        sha256: encoded.sha256().to_owned(),
        bytes: encoded.len() as i64,
        width: encoded.dimensions().0 as i32,
        height: encoded.dimensions().1 as i32,
    };
    let guard = store.try_lock()?;
    let pending = queue.prepare_output(job_id, token, &metadata).await?;
    let receipt = guard.install(pending.id.parse()?, &encoded)?;
    if receipt.sha256 != pending.sha256 || receipt.bytes != pending.bytes as u64 {
        return Err("output reservation differs from installed bytes".into());
    }
    // Failure/uncertain commit leaves a reserved object for retry or cleanup;
    // no error path removes a possibly approved file.
    Ok(queue.approve_output(job_id, token, &pending.id).await?)
}

pub async fn reconcile(queue: &MediaQueue, store: &PublicationStore) -> PublicationResult<u64> {
    let guard = store.try_lock()?;
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
