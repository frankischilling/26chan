//! Retained template-ban evidence for an already validated upload capability.
//! Missing normalized metadata is distinct from both no file and query failure.
use super::{AdmissionError, BoardSlug, Digest, ProviderValue};

/// Counts saturate at the only relevant threshold. The provider can stop each
/// indexed lookup after three rows, including duplicate historical events.
pub const THRESHOLD: u8 = 3;

#[derive(Clone, Copy)]
pub enum Upload {
    Absent,
    PresentUnknown,
    PresentKnown(Digest),
}

#[derive(Clone, Copy, Default)]
pub struct Counts {
    /// Known rows matching this upload's exact normalized digest.
    pub matching: u8,
    /// Known rows across all digests, including those matching this upload.
    pub known: u8,
    /// File-bearing bans whose normalized digest was not retained.
    pub unknown: u8,
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    /// The validated upload observed by the provider; None means a present
    /// legacy upload with unknown normalized metadata, never an absent file.
    pub digest: Option<Digest>,
    pub template_226: Counts,
    pub template_6: Counts,
}

impl Counts {
    fn valid(self, digest_known: bool) -> bool {
        self.matching <= self.known
            && self.known <= THRESHOLD
            && self.unknown <= THRESHOLD
            && (digest_known || self.matching == 0)
    }
}

/// All observations must come from the same locked private history snapshot.
/// A definite result does not require guessing the identity of unknown files.
pub fn evaluate(
    board: &str,
    worksafe: bool,
    upload: Upload,
    history: ProviderValue<Snapshot>,
) -> Result<bool, AdmissionError> {
    if BoardSlug::parse(board).is_err() {
        return Err(AdmissionError::InvalidPolicy);
    }
    if board == "b" || matches!(upload, Upload::Absent) {
        return Ok(false);
    }
    let ProviderValue::Available(history) = history else {
        return Err(AdmissionError::Unavailable);
    };
    let digest = match upload {
        Upload::PresentKnown(digest) => Some(digest),
        Upload::PresentUnknown => None,
        Upload::Absent => unreachable!("Absent uploads return before reading history"),
    };
    if history.digest != digest
        || !history.template_226.valid(digest.is_some())
        || !history.template_6.valid(digest.is_some())
    {
        return Err(AdmissionError::InvalidPolicy);
    }
    let a = history.template_226;
    let b = if worksafe {
        history.template_6
    } else {
        Counts::default()
    };
    let unknown = a.unknown + b.unknown;
    if digest.is_some() {
        let matches = a.matching + b.matching;
        if matches >= THRESHOLD {
            return Ok(true);
        }
        if matches + unknown < THRESHOLD {
            return Ok(false);
        }
    } else if a.known + b.known + unknown < THRESHOLD {
        // Even a legacy upload cannot match three events when fewer than
        // three potentially matching events exist anywhere in this history.
        return Ok(false);
    }
    Err(AdmissionError::Unavailable)
}

#[cfg(test)]
mod tests;
