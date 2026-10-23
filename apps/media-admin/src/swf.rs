//! Context-bound SWF publication. The guest cannot substitute the retained bytes.
use super::*;
use board_media::swf::ValidatedSwf;
use board_store::media::Job;

pub(crate) async fn dispatch_context(
    queue: &MediaQueue,
    store: &PublicationStore,
    client: &DispatchClient,
    job: &Job,
    snapshot: InputSnapshot,
) -> PublicationResult<Asset> {
    let token = job.lease_token.as_deref().ok_or("missing lease")?;
    let work = async {
        let disk = client.process_swf(snapshot.bytes()).await?;
        let output = ValidatedSwf::read_disk(disk.as_slice()).await?;
        let receipt = input_receipt(&snapshot);
        if output.bytes() != snapshot.bytes() {
            return Err("SWF worker changed the retained input".into());
        }
        let (width, height) = output.dimensions();
        let metadata = OutputMetadata {
            sha256: output.sha256().into(),
            bytes: output.len().try_into()?,
            width: width.try_into()?,
            height: height.try_into()?,
        };
        let guard = store.try_lock()?;
        let asset = queue
            .prepare_swf_output(&job.id, token, &metadata, &receipt)
            .await?;
        queue
            .record_input_receipt(&job.id, token, &asset.id, &receipt)
            .await?;
        let installed = guard.install_swf(asset.id.parse()?, &output)?;
        if installed.sha256 != asset.sha256
            || installed.bytes != asset.bytes as u64
            || asset.output_format != MediaFormat::Swf
        {
            return Err("SWF publication receipt mismatch".into());
        }
        queue
            .approve_output_with_input(&job.id, token, &asset.id, &receipt)
            .await
            .map_err(Into::into)
    };
    tokio::time::timeout(std::time::Duration::from_secs(29), work)
        .await
        .map_err(|_| "SWF processing deadline exceeded")?
}
