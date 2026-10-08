//! Immutable paired snapshot. Descriptor hashes are checked against the same
//! bytes later sent to the guest; paths are never reopened for provenance.
use crate::{MediaError, ObjectId, paired};
use std::{fs::File, ops::Range};
use tokio::io::AsyncReadExt;

#[derive(Clone, Debug)]
pub struct PairedInputDescriptor {
    pub job_id: ObjectId,
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub image_bytes: u64,
    pub image_sha256: [u8; 32],
    pub replay: Option<(u64, [u8; 32])>,
}

pub struct PairedInputSnapshot {
    bytes: Box<[u8]>,
    image: Range<usize>,
    replay: paired::ReplayPresence,
    descriptor: PairedInputDescriptor,
}

impl PairedInputSnapshot {
    pub async fn read(file: File, descriptor: PairedInputDescriptor) -> Result<Self, MediaError> {
        let header = paired::InputHeader::new(
            descriptor.job_id.bytes(),
            descriptor.image_bytes,
            descriptor.replay.map(|(bytes, _)| bytes),
        )?;
        if descriptor.bytes != header.total_bytes() {
            return Err(MediaError::InputLengthMismatch);
        }
        let mut file = tokio::fs::File::from_std(file);
        let metadata = file.metadata().await?;
        if !metadata.is_file() || metadata.len() != descriptor.bytes {
            return Err(MediaError::InputLengthMismatch);
        }
        let mut bytes = vec![0; descriptor.bytes as usize].into_boxed_slice();
        file.read_exact(&mut bytes).await?;
        let mut extra = [0];
        if file.read(&mut extra).await? != 0 {
            return Err(MediaError::InputLengthMismatch);
        }
        let input = paired::decode_input(
            paired::InputKind::PairedV2,
            &bytes,
            descriptor.job_id.bytes(),
        )?;
        if *input.frame_sha256() != descriptor.sha256
            || input.image_bytes().len() as u64 != descriptor.image_bytes
            || *input.image_sha256() != descriptor.image_sha256
            || input.replay_upload_bytes().map(|b| b.len() as u64)
                != descriptor.replay.map(|(len, _)| len)
            || input.replay_sha256().copied() != descriptor.replay.map(|(_, digest)| digest)
        {
            return Err(MediaError::InputLengthMismatch);
        }
        let image =
            paired::INPUT_HEADER_BYTES..paired::INPUT_HEADER_BYTES + input.image_bytes().len();
        let replay = input.replay_presence();
        Ok(Self {
            bytes,
            image,
            replay,
            descriptor,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn image_bytes(&self) -> &[u8] {
        &self.bytes[self.image.clone()]
    }
    pub fn replay_presence(&self) -> paired::ReplayPresence {
        self.replay
    }
    pub fn descriptor(&self) -> &PairedInputDescriptor {
        &self.descriptor
    }

    /// Fresh OS randomness is independent of all bearer and lease tokens.
    pub fn new_attempt_binding(&self) -> Result<[u8; 32], MediaError> {
        use sha2::{Digest, Sha256};
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| MediaError::Random)?;
        let mut digest = Sha256::new();
        digest.update(b"26chan-paired-attempt-v2\0");
        digest.update(self.descriptor.job_id.bytes());
        digest.update(nonce);
        digest.update(self.descriptor.sha256);
        Ok(digest.finalize().into())
    }
}
