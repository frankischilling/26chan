//! Server-selected input bounds; this value grants no staff authority.
use crate::{MAX_COMMENT_BYTES, MAX_COMMENT_CHARS, MAX_PUBLIC_FIELD_BYTES, ValidationError};

pub const MAX_AUTHORIZED_COMMENT_CHARS: usize = 50_000;
pub const MAX_AUTHORIZED_FIELD_BYTES: usize = 255;

#[derive(Clone, Copy)]
pub struct PostLimits {
    field_bytes: usize,
    comment_chars: usize,
    input_bytes: usize,
    prepared_chars: usize,
    prepared_bytes: usize,
    subject_bytes: usize,
    authorized: bool,
}

impl PostLimits {
    pub fn ordinary(max_chars: usize) -> Self {
        Self {
            field_bytes: MAX_PUBLIC_FIELD_BYTES,
            comment_chars: max_chars.min(MAX_COMMENT_CHARS),
            input_bytes: MAX_COMMENT_BYTES,
            prepared_chars: MAX_COMMENT_CHARS,
            prepared_bytes: MAX_COMMENT_BYTES,
            subject_bytes: MAX_PUBLIC_FIELD_BYTES * 4,
            authorized: false,
        }
    }

    pub fn authorized(max_chars: usize) -> Result<Self, ValidationError> {
        if !(1..=MAX_AUTHORIZED_COMMENT_CHARS).contains(&max_chars) {
            return Err(ValidationError(
                "Authorized posting limits are unavailable.",
            ));
        }
        Ok(Self {
            field_bytes: MAX_AUTHORIZED_FIELD_BYTES,
            comment_chars: max_chars,
            input_bytes: max_chars * 4,
            prepared_chars: max_chars * 4,
            prepared_bytes: max_chars * 4,
            subject_bytes: MAX_AUTHORIZED_FIELD_BYTES * 4,
            authorized: true,
        })
    }

    pub fn field_bytes(self) -> usize {
        self.field_bytes
    }
    pub fn comment_chars(self) -> usize {
        self.comment_chars
    }
    pub fn is_authorized(self) -> bool {
        self.authorized
    }
    pub fn validate_field(self, value: &str) -> Result<(), ValidationError> {
        if value.len() > self.field_bytes {
            return Err(ValidationError("Name or subject is too long."));
        }
        Ok(())
    }
    pub(crate) fn input_bytes(self) -> usize {
        self.input_bytes
    }
    pub(crate) fn prepared_chars(self) -> usize {
        self.prepared_chars
    }
    pub(crate) fn prepared_bytes(self) -> usize {
        self.prepared_bytes
    }
    pub(crate) fn subject_bytes(self) -> usize {
        self.subject_bytes
    }
    pub(crate) fn ordinary_checks(self) -> bool {
        !self.authorized
    }
}
