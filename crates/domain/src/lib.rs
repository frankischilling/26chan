#![forbid(unsafe_code)]

pub mod formatting;
pub use formatting::{Line, Token, parse_comment};

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
    max_bytes: usize,
) -> Result<(), ValidationError> {
    if name.len() > 80 || subject.len() > 120 {
        return Err(ValidationError("Name or subject is too long."));
    }
    if comment.trim().is_empty() || comment.len() > max_bytes.min(16_000) {
        return Err(ValidationError(
            "Enter a comment within this board's byte limit.",
        ));
    }
    if [name, subject, comment].iter().any(|s| {
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
    fn post_limits_are_bytes_and_reject_blank_comments() {
        assert!(validate_post("A", "subject", "hello", 20).is_ok());
        assert!(validate_post("A", "", "   \n", 20).is_err());
        assert!(validate_post("A", "", "ééé", 5).is_err());
        assert!(validate_post("A\0", "", "hello", 20).is_err());
    }
}
