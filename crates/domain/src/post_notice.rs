//! Staff-issued post notices are escaped display text, never user comment markup.
//! Selection does not authorize issuance; the database must revalidate staff rights.
use crate::ValidationError;

pub const WARNED: &str = "USER WAS WARNED FOR THIS POST";
pub const BANNED: &str = "USER WAS BANNED FOR THIS POST";
pub const MAX_NOTICE_BYTES: usize = 4096;
pub const MAX_NOTICES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice(String);
impl Notice {
    pub fn from_saved(text: &str) -> Result<Self, ValidationError> {
        if text.is_empty() || text.len() > MAX_NOTICE_BYTES || text.contains('\0') {
            return Err(ValidationError("Post notice is unavailable."));
        }
        Ok(Self(text.into()))
    }
    pub fn text(&self) -> &str {
        &self.0
    }
    pub fn source_html(&self) -> String {
        format!(
            " <br><br><strong style=\"color: red;\">({})</strong>",
            crate::source_html_entities(&self.0)
        )
    }
}

/// `custom_allowed` is the already-qualified manager/global-banmsg predicate.
/// An unprivileged custom field is ignored, matching the source default path.
pub fn select(
    enabled: bool,
    warning: bool,
    custom_allowed: bool,
    custom: Option<&str>,
) -> Result<Option<Notice>, ValidationError> {
    if !enabled {
        return Ok(None);
    }
    let text = if custom_allowed {
        custom.filter(|value| !value.is_empty())
    } else {
        None
    }
    .unwrap_or(if warning { WARNED } else { BANNED });
    Notice::from_saved(text).map(Some)
}

/// The source auto-clear is distinct from weighted moderator group clearing.
pub fn clears_reports(post_action: &str) -> bool {
    !matches!(post_action, "delpost" | "move" | "archive")
}

#[cfg(test)]
mod tests;
