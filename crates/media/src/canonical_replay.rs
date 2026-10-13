//! Owned canonical replay bytes with independent structure and numeric checks.
//! This is not proof of logical history, raster agreement, or safe execution.
//! Publication still needs a current paired lease and matching database receipt;
//! playback requires the isolated consumer and its independent resource limits.
use crate::{MediaError, replay_envelope, replay_wire};
use sha2::{Digest, Sha256};

pub struct CanonicalReplay {
    bytes: Vec<u8>,
    sha256: String,
    dimensions: (u16, u16),
}
impl CanonicalReplay {
    pub fn check(bytes: Vec<u8>) -> Result<Self, MediaError> {
        let replay = replay_wire::decode(&bytes).map_err(|_| MediaError::InvalidOutput)?;
        replay_envelope::check(&replay).map_err(|_| MediaError::InvalidOutput)?;
        Ok(Self {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            dimensions: (replay.metadata.width, replay.metadata.height),
            bytes,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn dimensions(&self) -> (u16, u16) {
        self.dimensions
    }
}
