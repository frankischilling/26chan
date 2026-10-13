//! Retained document bytes are untrusted content, even after isolated processing.
//! They may only be served on the restricted document/media origin. No PDF
//! parser, metadata reader or interpreter belongs in this host module.
use crate::{InputSnapshot, MAX_INPUT_BYTES, MediaError, ValidatedOutput};
use md5::{Digest, Md5};

pub const MAX_PDF_BYTES: u64 = MAX_INPUT_BYTES;

pub struct RetainedPdf {
    snapshot: InputSnapshot,
    preview: ValidatedOutput,
    sha256: String,
    md5: String,
}

impl RetainedPdf {
    /// The trusted coordinator calls this after the explicit PDF job finishes
    /// and its stopped pixel output passes validation. Keep the exact snapshot
    /// sent to that job; never reopen the intake pathname or trust guest-supplied
    /// document bytes, paths, hashes or sizes.
    pub fn from_processed_snapshot(
        snapshot: InputSnapshot,
        preview: ValidatedOutput,
    ) -> Result<Self, MediaError> {
        if !snapshot.bytes().starts_with(b"%PDF-") {
            return Err(MediaError::InvalidOutput);
        }
        let sha256 = snapshot
            .raw_sha256()
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect();
        let md5 = Md5::digest(snapshot.bytes())
            .iter()
            .map(|value| format!("{value:02x}"))
            .collect();
        Ok(Self {
            snapshot,
            preview,
            sha256,
            md5,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        self.snapshot.bytes()
    }

    pub fn len(&self) -> u64 {
        self.bytes().len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Original-file interoperability only, never authority or integrity.
    pub fn md5(&self) -> &str {
        &self.md5
    }

    pub fn preview(&self) -> &ValidatedOutput {
        &self.preview
    }
}
