#![forbid(unsafe_code)]

pub mod bump;
pub mod formatting;
pub mod posting_options;
pub use formatting::{Line, Token, parse_comment};

pub const MAX_COMMENT_CHARS: usize = 16_000;
pub const MAX_COMMENT_BYTES: usize = 64_000;
pub const MAX_PUBLIC_FIELD_BYTES: usize = 100;

/// The posting reference converts CRLF and lone CR to LF before length checks.
/// Bound input before allocating; normalization never increases its byte size.
pub fn normalize_comment(value: &str) -> Result<std::borrow::Cow<'_, str>, ValidationError> {
    if value.len() > MAX_COMMENT_BYTES {
        return Err(ValidationError(
            "Enter a comment within this board's character limit.",
        ));
    }
    if !value.contains('\r') {
        return Ok(std::borrow::Cow::Borrowed(value));
    }
    let mut normalized = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(ch);
        }
    }
    Ok(std::borrow::Cow::Owned(normalized))
}

/// Original tail eligibility uses visible replies; sticky alone does not double it.
pub fn thread_tail_size(configured: u16, sticky: bool, undead: bool, replies: usize) -> usize {
    let size = usize::from(configured) * if sticky && undead { 2 } else { 1 };
    if size > 0 && replies >= size * 2 {
        size
    } else {
        0
    }
}

#[test]
fn native_tail_threshold_and_sticky_undead_rules() {
    for (size, sticky, undead, replies, expected) in [
        (0, false, false, 1000, 0),
        (5, false, false, 9, 0),
        (5, false, false, 10, 5),
        (50, true, false, 100, 50),
        (50, false, true, 100, 50),
        (50, true, true, 199, 0),
        (50, true, true, 200, 100),
        (500, false, false, 999, 0),
        (500, false, false, 1000, 500),
    ] {
        assert_eq!(thread_tail_size(size, sticky, undead, replies), expected);
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(pub &'static str);

#[derive(Clone, Debug)]
pub struct BoardSlug(String);

impl BoardSlug {
    pub fn parse(value: &str) -> Result<Self, ValidationError> {
        if value.is_empty()
            || value.len() > 10
            || !value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            return Err(ValidationError(
                "Board names must contain 1 to 10 lowercase letters or digits.",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn validate_post(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
) -> Result<(), ValidationError> {
    validate_post_with_attachment(name, subject, comment, max_chars, false)
}

/// Validate requested content, not attachment authority. The store must still
/// authorize and insert any attachment atomically before committing the post.
pub fn validate_post_with_attachment(
    name: &str,
    subject: &str,
    comment: &str,
    max_chars: usize,
    has_attachment: bool,
) -> Result<(), ValidationError> {
    if name.len() > MAX_PUBLIC_FIELD_BYTES || subject.len() > MAX_PUBLIC_FIELD_BYTES {
        return Err(ValidationError("Name or subject is too long."));
    }
    let comment = normalize_comment(comment)?;
    if (comment.trim().is_empty() && !(has_attachment && comment.is_empty()))
        || comment.chars().count() > max_chars.min(MAX_COMMENT_CHARS)
    {
        return Err(ValidationError(
            "Enter a comment within this board's character limit.",
        ));
    }
    if [name, subject, comment.as_ref()].iter().any(|s| {
        s.chars()
            .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    }) {
        return Err(ValidationError("Unsupported control character."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_names_cannot_be_paths_or_identifiers() {
        assert!(BoardSlug::parse("tech").is_ok());
        for value in ["", "../a", "a/b", "a'", "Admin", "abcdefghijk"] {
            assert!(BoardSlug::parse(value).is_err(), "{value}");
        }
    }

    #[test]
    fn formatting_produces_typed_nodes_without_accepting_html() {
        let lines = parse_comment(
            ">hello <script>\n>>42 [spoiler]secret[/spoiler] https://example.org/ javascript:alert(1)",
        );
        assert!(lines[0].green);
        assert_eq!(lines[0].tokens, vec![Token::Text(">hello <script>".into())]);
        assert!(lines[1].tokens.contains(&Token::Quote(42)));
        assert!(lines[1].tokens.contains(&Token::Spoiler("secret".into())));
        assert!(
            lines[1]
                .tokens
                .iter()
                .any(|t| matches!(t, Token::Link(url) if url == "https://example.org/"))
        );
        assert!(
            !lines[1]
                .tokens
                .iter()
                .any(|t| matches!(t, Token::Link(url) if url.starts_with("javascript:")))
        );
    }

    #[test]
    fn post_limits_count_unicode_scalars_and_reject_blank_comments() {
        assert!(validate_post("A", "subject", "hello", 20).is_ok());
        assert!(validate_post("A", "", "   \n", 20).is_err());
        assert!(validate_post("A", "", &"é".repeat(4), 4).is_ok());
        assert!(validate_post("A", "", &"😀".repeat(4), 4).is_ok());
        assert!(validate_post("A", "", "e\u{301}", 2).is_ok());
        assert!(validate_post("A", "", "e\u{301}", 1).is_err());
        assert!(validate_post("A", "", "a\r\nb", 4).is_ok());
        assert!(validate_post("A", "", "a\r\nb", 3).is_ok());
        assert!(validate_post("A", "", "a\r\nb", 2).is_err());
        assert!(validate_post("A", "", &"x".repeat(16_001), 16_000).is_err());
        assert!(validate_post("A", "", &"😀".repeat(16_000), 16_000).is_ok());
        assert!(validate_post("A", "", &"😀".repeat(16_001), 16_000).is_err());
        assert!(validate_post("A\0", "", "hello", 20).is_err());
    }
}
