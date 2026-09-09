#![forbid(unsafe_code)]

//! Bounded private intake and encoder-only output promotion. This crate does not
//! execute workers, decode uploads, expose public routes, or establish process
//! isolation. Deployment permissions and job deadlines belong to the caller.

mod block;
mod id;
mod output;
mod promotion;
mod publication;
mod quarantine;

pub use block::{OUTPUT_DISK_BYTES, write_input_disk};
pub use id::ObjectId;
pub use output::{EncodedOutput, ValidatedOutput};
pub use promotion::{Promoter, Promotion};
pub use publication::{ApprovedFiles, PublicationGuard, PublicationStore};
pub use quarantine::Quarantine;

pub const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 1024;
pub const MAX_PNG_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("invalid object ID")]
    InvalidId,
    #[error("operating system randomness unavailable")]
    Random,
    #[error("empty input")]
    Empty,
    #[error("input exceeds byte limit")]
    InputTooLarge,
    #[error("input length does not match declared byte count")]
    InputLengthMismatch,
    #[error("object already exists")]
    AlreadyExists,
    #[error("invalid pixel protocol")]
    InvalidOutput,
    #[error("public and quarantine directories must not overlap")]
    OverlappingRoots,
    #[error("published object conflicts with output")]
    Conflict,
    #[error("another publication or cleanup owns the storage lock")]
    Busy,
    #[error("invalid publication storage entry")]
    InvalidStorage,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Encoding(#[from] png::EncodingError),
}
