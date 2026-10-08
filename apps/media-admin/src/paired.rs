//! Inactive paired candidate coordinator. No CLI, route, admission, encoder,
//! storage or publication path invokes this API. A real SQL lease is required.
use crate::PublicationResult;
use board_media::{
    ObjectId, Quarantine,
    paired::{self, InputKind, ReplayPresence},
    paired_snapshot::{PairedInputDescriptor, PairedInputSnapshot},
    source_digest::{PngSourceDigestLimits, SourceDigestProfile, png_source_processed_digest},
};
use board_media_dispatch::DispatchClient;
use board_store::{
    media::MediaQueue,
    media_assets::{SourceProfile, SourceProvenance},
    paired_candidates::PairedCandidateLease,
};

/// Host-checked joint transport candidate. This has no publication conversion.
/// The provenance describes only the PNG source component, not the bundle.
pub struct CheckedPairedCandidate {
    job_id: ObjectId,
    binding: [u8; 32],
    presence: ReplayPresence,
    disk: Vec<u8>,
    png_source: SourceProvenance,
    bundle_sha256: [u8; 32],
}
impl CheckedPairedCandidate {
    pub fn job_id(&self) -> ObjectId {
        self.job_id
    }
    pub fn bundle_sha256(&self) -> &[u8; 32] {
        &self.bundle_sha256
    }
    pub fn png_source(&self) -> &SourceProvenance {
        &self.png_source
    }
    pub fn candidate(&self) -> Result<paired::PairedResultCandidate<'_>, paired::PairedError> {
        paired::decode_result(InputKind::PairedV2, &self.disk, self.binding, self.presence)
    }
}

/// Claim exactly one paired job through its typed SQL boundary, snapshot its
/// recorded object once, and independently check the disposable guest output.
/// A successful lease completion records candidate_checked, not publication.
pub async fn dispatch_candidate(
    queue: &MediaQueue,
    quarantine: &Quarantine,
    client: &DispatchClient,
) -> PublicationResult<CheckedPairedCandidate> {
    let lease = queue
        .claim_paired_candidate()
        .await?
        .ok_or("no paired candidate available")?;
    let processed = tokio::time::timeout(std::time::Duration::from_secs(29), async {
        let descriptor = descriptor(&lease)?;
        let file = quarantine.open_paired_input(descriptor.job_id, descriptor.bytes)?;
        let snapshot = PairedInputSnapshot::read(file, descriptor).await?;
        let png_source = png_provenance(&snapshot)?;
        let binding = snapshot.new_attempt_binding()?;
        let disk = client
            .process_paired(snapshot.bytes(), snapshot.bytes().len() as u64, &binding)
            .await?;
        // Independently parses all guest-controlled lengths, flags, binding,
        // padding, RGBA dimensions and canonical replay wire. No state/cost
        // success is interpreted as replay admission or faithful rendering.
        paired::decode_result(
            InputKind::PairedV2,
            &disk,
            binding,
            snapshot.replay_presence(),
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(CheckedPairedCandidate {
            job_id: snapshot.descriptor().job_id,
            binding,
            presence: snapshot.replay_presence(),
            disk,
            png_source,
            bundle_sha256: snapshot.descriptor().sha256,
        })
    })
    .await;
    match processed {
        Ok(Ok(candidate)) => {
            // Stale or uncertain completion never returns a successful candidate.
            queue.finish_paired_candidate(lease, true).await?;
            Ok(candidate)
        }
        _ => {
            let _ = queue.finish_paired_candidate(lease, false).await;
            Err("paired candidate processing rejected".into())
        }
    }
}

fn descriptor(lease: &PairedCandidateLease) -> PublicationResult<PairedInputDescriptor> {
    let bytes = |n: Option<i64>| -> PublicationResult<u64> {
        Ok(n.ok_or("missing paired length")?.try_into()?)
    };
    let replay = match (lease.replay_bytes(), lease.replay_sha256()) {
        (None, None) => None,
        (Some(len), Some(digest)) => Some((len.try_into()?, sha256(digest)?)),
        _ => return Err("incomplete paired replay descriptor".into()),
    };
    Ok(PairedInputDescriptor {
        job_id: lease.id().parse()?,
        bytes: bytes(lease.input_bytes())?,
        sha256: sha256(lease.input_sha256().ok_or("missing bundle digest")?)?,
        image_bytes: bytes(lease.image_bytes())?,
        image_sha256: sha256(lease.image_sha256().ok_or("missing PNG digest")?)?,
        replay,
    })
}

fn sha256(value: &str) -> PublicationResult<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid paired digest".into());
    }
    let mut digest = [0; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
    }
    Ok(digest)
}
fn png_provenance(snapshot: &PairedInputSnapshot) -> PublicationResult<SourceProvenance> {
    let cap = paired::MAX_PNG_INPUT_BYTES as usize;
    let digest = png_source_processed_digest(
        snapshot.image_bytes(),
        PngSourceDigestLimits::new(cap, cap, cap)?,
    )?;
    Ok(SourceProvenance {
        input_sha256: snapshot
            .descriptor()
            .image_sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        input_bytes: snapshot.image_bytes().len() as i64,
        profile: match digest.profile() {
            SourceDigestProfile::PngV1 => SourceProfile::PngV1,
        },
        retained_bytes: digest.processed_bytes() as i64,
        md5: *digest.md5(),
    })
}
