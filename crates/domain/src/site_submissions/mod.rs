//! Visible site-submission contracts and bounded native validation.
//! The supplied tree contains templates, not the feedback/application/contest
//! controllers. Native limits and ownership rules must not be called source
//! controller equivalence.
pub mod applications;
pub mod contests;
pub mod feedback;
pub mod ownership;

use crate::ValidationError;

/// Match browser maxlength's UTF-16 units while bounding UTF-8 allocation.
/// These fields are text, never trusted HTML. Storage/rendering escape separately.
pub fn text(
    raw: &str,
    maximum: usize,
    multiline: bool,
    required: bool,
) -> Result<String, ValidationError> {
    if maximum == 0
        || maximum > 16_000
        || raw.len() > maximum * 4
        || raw.encode_utf16().count() > maximum
        || raw
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
        || required && raw.trim().is_empty()
    {
        return Err(ValidationError("Invalid submission field."));
    }
    Ok(raw.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Contact addresses need not belong to the public sign-in provider allowlist.
/// Keep the display address separate from the case-folded duplicate identity.
pub struct ContactEmail {
    recipient: String,
    identity: String,
}
impl ContactEmail {
    pub fn parse(raw: &str) -> Result<Self, ValidationError> {
        const BAD: ValidationError = ValidationError("Invalid contact email.");
        if raw.len() > 320 {
            return Err(BAD);
        }
        let value = raw.trim_matches([' ', '\t']);
        if value.len() > 254 || !value.is_ascii() {
            return Err(BAD);
        }
        let (local, domain) = value.split_once('@').ok_or(BAD)?;
        if local.is_empty()
            || local.len() > 64
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
            || !domain.contains('.')
            || domain.len() > 253
            || domain.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(BAD);
        }
        Ok(Self {
            recipient: value.to_owned(),
            identity: value.to_ascii_lowercase(),
        })
    }
    pub fn recipient(&self) -> &str {
        &self.recipient
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_is_bounded_uninterpreted_and_uses_browser_length_units() {
        assert_eq!(text("<script>&", 20, false, true).unwrap(), "<script>&");
        assert_eq!(text("a\r\nb\rc", 20, true, true).unwrap(), "a\nb\nc");
        assert!(text("😀", 2, false, true).is_ok());
        assert!(text("😀", 1, false, true).is_err());
        for value in ["", "  ", "\n", "a\0b"] {
            assert!(text(value, 20, false, true).is_err());
        }
        assert!(text("", 20, false, false).is_ok());
        assert!(text("x", 16_001, false, true).is_err());
    }
    #[test]
    fn contact_email_has_no_login_provider_policy_or_debug_representation() {
        let email = ContactEmail::parse("  Owned+Alias@Example.ORG\t").unwrap();
        assert_eq!(email.recipient(), "Owned+Alias@Example.ORG");
        assert_eq!(email.identity(), "owned+alias@example.org");
        for value in [
            "",
            "x",
            "x@local",
            "x@-example.org",
            "x@example..org",
            "x\r\nBcc:y@example.org",
            "a..b@example.org",
            "x@y@example.org",
        ] {
            assert!(ContactEmail::parse(value).is_err(), "{value:?}");
        }
    }
}
