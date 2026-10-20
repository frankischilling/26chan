//! Public duplicate-file refusal. IDs are returned only after capability checks.
#[derive(Debug, thiserror::Error)]
#[error("Error: Duplicate file exists.")]
pub struct ImageDuplicateRefusal {
    pub existing: Option<(i64, i64)>,
}
