//! Finite saved-format budgets. Database authority controls staff format use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordfilterLimits {
    Ordinary,
    Authorized,
}

impl WordfilterLimits {
    pub fn for_post(limits: crate::PostLimits) -> Self {
        if limits.is_authorized() {
            Self::Authorized
        } else {
            Self::Ordinary
        }
    }
    pub fn input_bytes(self) -> usize {
        match self {
            Self::Ordinary => 131_072,
            Self::Authorized => 524_288,
        }
    }
    pub fn output_bytes(self) -> usize {
        match self {
            Self::Ordinary => 524_288,
            Self::Authorized => 2_097_152,
        }
    }
    pub fn stored_bytes(self) -> usize {
        match self {
            Self::Ordinary => 131_072,
            Self::Authorized => 524_288,
        }
    }

    pub fn saved_post_read_bytes(self) -> usize {
        match self {
            Self::Ordinary => crate::MAX_COMMENT_BYTES,
            Self::Authorized => self.output_bytes() + self.stored_bytes() * 2,
        }
    }
    pub(crate) fn max_parts(self) -> usize {
        match self {
            Self::Ordinary => 32_768,
            Self::Authorized => 131_072,
        }
    }
    pub(crate) fn version(self) -> &'static [u8; 4] {
        match self {
            Self::Ordinary => b"WF01",
            Self::Authorized => b"WF02",
        }
    }
}
