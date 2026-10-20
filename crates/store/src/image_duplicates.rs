//! Public duplicate-file refusals. IDs are returned only after capability checks.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImageDuplicateRefusal {
    #[error("Error: Duplicate file exists.")]
    Duplicate { existing: Option<(i64, i64)> },
    #[error("Error: You must wait longer before reposting this file.")]
    RecentUpload,
}

impl ImageDuplicateRefusal {
    pub fn existing(&self) -> Option<(i64, i64)> {
        match self {
            Self::Duplicate { existing } => *existing,
            Self::RecentUpload => None,
        }
    }

    pub fn source_message(&self) -> &'static str {
        match self {
            Self::Duplicate { .. } => board_domain::image_duplicates::DUPLICATE,
            Self::RecentUpload => board_domain::image_duplicates::RECENT_UPLOAD,
        }
    }
}
