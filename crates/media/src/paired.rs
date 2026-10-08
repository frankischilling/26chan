//! Isolated, non-authoritative paired-input and paired-result framing candidates.
//!
//! These pure byte-slice APIs are not intake, a dispatch protocol, or a guest
//! command. Callers must explicitly select v2; no magic sniffing or v1 fallback
//! is provided. The existing image-v1 APIs and constants are unchanged.
//!
//! Parsing input establishes only exact framing and comparison with a supplied
//! job ID. It does not establish outer HTTP completion, a persisted job record,
//! a sealed snapshot, image validity, replay validity, or source provenance.
//! Component and frame SHA-256 values describe actual bytes only. In particular,
//! a frame fingerprint is NOT a fresh per-attempt result binding.
//!
//! Results establish bounded RGBA bytes and, if present, structurally decoded
//! replay data. They establish no faithful transformation, image/replay pixel
//! equality, state or cost qualification, storage permission, or publication
//! authority. There is no conversion to `ValidatedOutput` or encoder API.
//!
//! The caller owns bounded acquisition, deadlines, outer transport completion,
//! and any future trusted job/attempt association. Slice length is not proof of
//! completion of a larger stream. All multibyte wire integers are big-endian.

use crate::{MAX_DIMENSION, MAX_INPUT_BYTES, replay_wire};
use replay_wire::{ReplayWireError, UntrustedReplay};
use sha2::{Digest, Sha256};

pub const VERSION: u16 = 2;
pub const INPUT_HEADER_BYTES: usize = 48;
pub const INPUT_TRAILER_BYTES: usize = 8;
pub const RESULT_HEADER_BYTES: usize = 64;
/// Candidate v2 result size only; never replaces the v1 output disk size.
pub const RESULT_BYTES: usize = 4_456_960;
const PIXEL_HEADER_BYTES: usize = 16;
const HAS_REPLAY: u32 = 1;

/// Explicit caller selection, never inferred from bytes. The v1 variant exists
/// only to reject misrouted calls; this module does not process image-v1 input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKind {
    ImageV1,
    PairedV2,
}

/// Absence is distinct from a present upload/wire value with zero bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPresence {
    Absent,
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PairedError {
    #[error("paired framing requires explicit paired-v2 selection")]
    Kind,
    #[error("paired input is outside the whole-job byte limit")]
    InputSize,
    #[error("paired result does not have the fixed v2 byte length")]
    ResultSize,
    #[error("unsupported paired framing header or trailer")]
    Header,
    #[error("unknown paired framing flag")]
    Flags,
    #[error("paired input image component is empty")]
    EmptyImage,
    #[error("replay presence disagrees with its byte count or expectation")]
    ReplayPresence,
    #[error("paired framing length is invalid or does not match the bytes")]
    Length,
    #[error("paired input job ID does not match the supplied expectation")]
    JobId,
    #[error("paired result binding does not match the supplied expectation")]
    Binding,
    #[error("invalid compact RGBA header, dimensions, or exact length")]
    Pixels,
    #[error("paired result padding is nonzero")]
    Padding,
    #[error("paired framing allocation failed")]
    Allocation,
    #[error(transparent)]
    Replay(#[from] ReplayWireError),
}

/// Borrowed, unvalidated image/replay uploads with exact candidate framing.
/// No completed-intake, provenance, or persisted-job authority is represented.
#[derive(Debug)]
pub struct PairedInputCandidate<'a> {
    job_id: [u8; 16],
    image: &'a [u8],
    replay_upload: Option<&'a [u8]>,
    frame_sha256: [u8; 32],
    image_sha256: [u8; 32],
    replay_sha256: Option<[u8; 32]>,
}

impl<'a> PairedInputCandidate<'a> {
    pub fn job_id(&self) -> &[u8; 16] {
        &self.job_id
    }

    /// Arbitrary bytes, not a host-validated PNG. A future guest must decode it.
    pub fn image_bytes(&self) -> &'a [u8] {
        self.image
    }

    /// Arbitrary bytes, not parsed TGKR or a qualified replay.
    pub fn replay_upload_bytes(&self) -> Option<&'a [u8]> {
        self.replay_upload
    }

    pub fn replay_presence(&self) -> ReplayPresence {
        presence(self.replay_upload.is_some())
    }

    /// Actual frame fingerprint only, never a fresh attempt binding.
    pub fn frame_sha256(&self) -> &[u8; 32] {
        &self.frame_sha256
    }

    pub fn image_sha256(&self) -> &[u8; 32] {
        &self.image_sha256
    }

    pub fn replay_sha256(&self) -> Option<&[u8; 32]> {
        self.replay_sha256.as_ref()
    }
}

/// Framing-checked result candidate, with no approval or encoder conversion.
#[derive(Debug)]
pub struct PairedResultCandidate<'a> {
    binding: [u8; 32],
    dimensions: (u32, u32),
    rgba: &'a [u8],
    replay_wire: Option<&'a [u8]>,
    replay: Option<UntrustedReplay>,
}

impl<'a> PairedResultCandidate<'a> {
    /// Compared with a caller-supplied value; this is not independently attested.
    pub fn binding(&self) -> &[u8; 32] {
        &self.binding
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    /// Bounded pixels only, with no claim of correspondence to any source.
    pub fn rgba_bytes(&self) -> &'a [u8] {
        self.rgba
    }

    pub fn replay_wire_bytes(&self) -> Option<&'a [u8]> {
        self.replay_wire
    }

    /// Structural replay decoding only; state and cost checks remain separate.
    pub fn untrusted_replay(&self) -> Option<&UntrustedReplay> {
        self.replay.as_ref()
    }
}

/// Encode a candidate frame, with the header, components, and trailer sharing
/// the existing 8 MiB job cap. No image/replay decoding or storage occurs.
/// `Some(&[])` is rejected, never normalized to absent replay.
pub fn encode_input(
    kind: InputKind,
    job_id: [u8; 16],
    image: &[u8],
    replay_upload: Option<&[u8]>,
) -> Result<Vec<u8>, PairedError> {
    require_kind(kind)?;
    let image_len = u64::try_from(image.len()).map_err(|_| PairedError::InputSize)?;
    let replay_len =
        u64::try_from(replay_upload.map_or(0, <[u8]>::len)).map_err(|_| PairedError::InputSize)?;
    let has_replay = replay_upload.is_some();
    let length = input_length(image_len, replay_len, has_replay)?;
    let mut frame = Vec::new();
    frame
        .try_reserve_exact(length)
        .map_err(|_| PairedError::Allocation)?;
    frame.extend_from_slice(b"IBPAIR02");
    frame.extend_from_slice(&VERSION.to_be_bytes());
    frame.extend_from_slice(&(INPUT_HEADER_BYTES as u16).to_be_bytes());
    frame.extend_from_slice(&u32::from(has_replay).to_be_bytes());
    frame.extend_from_slice(&image_len.to_be_bytes());
    frame.extend_from_slice(&replay_len.to_be_bytes());
    frame.extend_from_slice(&job_id);
    frame.extend_from_slice(image);
    if let Some(replay) = replay_upload {
        frame.extend_from_slice(replay);
    }
    frame.extend_from_slice(b"IBDONE02");
    Ok(frame)
}

/// Parse exactly one candidate frame, including the trailer and no padding or
/// trailing bytes. `expected_job_id` is merely compared, not authenticated here.
pub fn decode_input(
    kind: InputKind,
    input: &[u8],
    expected_job_id: [u8; 16],
) -> Result<PairedInputCandidate<'_>, PairedError> {
    require_kind(kind)?;
    if input.len() < INPUT_HEADER_BYTES + INPUT_TRAILER_BYTES
        || u64::try_from(input.len()).map_err(|_| PairedError::InputSize)? > MAX_INPUT_BYTES
    {
        return Err(PairedError::InputSize);
    }
    require_header(input, b"IBPAIR02", INPUT_HEADER_BYTES)?;
    let has_replay = replay_flag(input)?;
    let image_len = u64_at(input, 16);
    let replay_len = u64_at(input, 24);
    if input_length(image_len, replay_len, has_replay)? != input.len() {
        return Err(PairedError::Length);
    }
    if input[32..48] != expected_job_id {
        return Err(PairedError::JobId);
    }
    let image_end = INPUT_HEADER_BYTES
        .checked_add(usize::try_from(image_len).map_err(|_| PairedError::Length)?)
        .ok_or(PairedError::Length)?;
    let replay_end = image_end
        .checked_add(usize::try_from(replay_len).map_err(|_| PairedError::Length)?)
        .ok_or(PairedError::Length)?;
    // Exact bounded total above proves these slices, without allocating input.
    if &input[replay_end..] != b"IBDONE02" {
        return Err(PairedError::Header);
    }
    let image = &input[INPUT_HEADER_BYTES..image_end];
    let replay_upload = has_replay.then_some(&input[image_end..replay_end]);
    Ok(PairedInputCandidate {
        job_id: expected_job_id,
        image,
        replay_upload,
        frame_sha256: Sha256::digest(input).into(),
        image_sha256: Sha256::digest(image).into(),
        replay_sha256: replay_upload.map(|bytes| Sha256::digest(bytes).into()),
    })
}

/// Decode an exact fixed-size candidate result. The caller supplies an opaque
/// expected per-attempt binding and the replay presence expected for that job.
/// This module neither creates nor authenticates that association; in particular
/// it never substitutes an input fingerprint for a fresh attempt binding.
pub fn decode_result(
    kind: InputKind,
    input: &[u8],
    expected_binding: [u8; 32],
    expected_replay: ReplayPresence,
) -> Result<PairedResultCandidate<'_>, PairedError> {
    require_kind(kind)?;
    if input.len() != RESULT_BYTES {
        return Err(PairedError::ResultSize);
    }
    require_header(input, b"IBRES002", RESULT_HEADER_BYTES)?;
    let has_replay = replay_flag(input)?;
    if presence(has_replay) != expected_replay {
        return Err(PairedError::ReplayPresence);
    }
    if input[16..48] != expected_binding {
        return Err(PairedError::Binding);
    }
    let pixel_len = usize::try_from(u64_at(input, 48)).map_err(|_| PairedError::Length)?;
    let replay_len = usize::try_from(u64_at(input, 56)).map_err(|_| PairedError::Length)?;
    if has_replay != (replay_len != 0) {
        return Err(PairedError::ReplayPresence);
    }
    if replay_len > replay_wire::MAX_WIRE_BYTES {
        return Err(PairedError::Length);
    }
    let pixel_end = RESULT_HEADER_BYTES
        .checked_add(pixel_len)
        .ok_or(PairedError::Length)?;
    let replay_end = pixel_end
        .checked_add(replay_len)
        .filter(|&end| end <= RESULT_BYTES)
        .ok_or(PairedError::Length)?;
    let (dimensions, rgba) = compact_pixels(&input[RESULT_HEADER_BYTES..pixel_end])?;
    if input[replay_end..].iter().any(|&byte| byte != 0) {
        return Err(PairedError::Padding);
    }
    let replay_wire = has_replay.then_some(&input[pixel_end..replay_end]);
    let replay = replay_wire.map(replay_wire::decode).transpose()?;
    // Each component has its own dimension bounds. Same-submission framing does
    // not imply matching canvas dimensions or visual correspondence.
    Ok(PairedResultCandidate {
        binding: expected_binding,
        dimensions,
        rgba,
        replay_wire,
        replay,
    })
}

fn require_kind(kind: InputKind) -> Result<(), PairedError> {
    if kind == InputKind::PairedV2 {
        Ok(())
    } else {
        Err(PairedError::Kind)
    }
}

fn presence(has_replay: bool) -> ReplayPresence {
    if has_replay {
        ReplayPresence::Present
    } else {
        ReplayPresence::Absent
    }
}

fn input_length(image: u64, replay: u64, has_replay: bool) -> Result<usize, PairedError> {
    if image == 0 {
        return Err(PairedError::EmptyImage);
    }
    if has_replay != (replay != 0) {
        return Err(PairedError::ReplayPresence);
    }
    let total = image
        .checked_add(replay)
        .and_then(|length| length.checked_add((INPUT_HEADER_BYTES + INPUT_TRAILER_BYTES) as u64))
        .ok_or(PairedError::Length)?;
    if total > MAX_INPUT_BYTES {
        return Err(PairedError::InputSize);
    }
    usize::try_from(total).map_err(|_| PairedError::InputSize)
}

// Callers first establish the complete fixed header.
fn require_header(input: &[u8], magic: &[u8; 8], length: usize) -> Result<(), PairedError> {
    if &input[..8] != magic
        || u16::from_be_bytes([input[8], input[9]]) != VERSION
        || usize::from(u16::from_be_bytes([input[10], input[11]])) != length
    {
        return Err(PairedError::Header);
    }
    Ok(())
}

fn replay_flag(input: &[u8]) -> Result<bool, PairedError> {
    let flags = u32_at(input, 12);
    if flags & !HAS_REPLAY != 0 {
        return Err(PairedError::Flags);
    }
    Ok(flags == HAS_REPLAY)
}

fn compact_pixels(input: &[u8]) -> Result<((u32, u32), &[u8]), PairedError> {
    if input.len() < PIXEL_HEADER_BYTES || &input[..8] != b"IBRGBA01" {
        return Err(PairedError::Pixels);
    }
    let width = u32_at(input, 8);
    let height = u32_at(input, 12);
    if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
        return Err(PairedError::Pixels);
    }
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(usize::try_from(height).ok()?))
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| bytes.checked_add(PIXEL_HEADER_BYTES))
        .ok_or(PairedError::Pixels)?;
    if input.len() != expected {
        return Err(PairedError::Pixels);
    }
    Ok(((width, height), &input[PIXEL_HEADER_BYTES..]))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"))
}
