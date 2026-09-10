#![forbid(unsafe_code)]

//! Trusted operator publication. The worker never receives this process's
//! credentials or storage authority. Public media remains disabled.
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_media_dispatch::DispatchClient;
use board_store::{
    media::{Failure, MediaQueue},
    media_assets::{Asset, MediaReader, OutputMetadata},
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
        client
            .process(tokio::fs::File::from_std(input), length)
            .await
            .map_err(|_| "dispatch processing failed")
    };
    let disk = match tokio::time::timeout(std::time::Duration::from_secs(29), processed).await {
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
    publish(queue, store, &job.id, token, &output)
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
