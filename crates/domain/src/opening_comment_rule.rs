//! COM_REGEX runs against the final source comment, before flood checks.
use crate::content_admission::{AdmissionError, BoundedRegex, MAX_PATTERN_BYTES};

/// Empty and exactly "0" are PHP-false and disable the source rule. Invalid
/// patterns and exhausted match budgets fail closed rather than bypassing it.
pub fn matches(pattern: &str, comment: &str) -> Result<bool, AdmissionError> {
    if pattern.is_empty() || pattern == "0" {
        return Ok(true);
    }
    if pattern.len() > MAX_PATTERN_BYTES || comment.len() > 2 * 1024 * 1024 {
        return Err(AdmissionError::InvalidPolicy);
    }
    BoundedRegex::compile(pattern)?.matches(comment, 1, &mut 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_every_executed_source_board_case() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/opening-comment-rules.json"))
                .unwrap();
        let cases = cases.as_array().unwrap();
        assert_eq!(cases.len(), 82 * 2 * 16);
        for case in cases {
            let allowed = case["parent"].as_i64().unwrap() != 0
                || matches(
                    case["pattern"].as_str().unwrap(),
                    case["comment"].as_str().unwrap(),
                )
                .unwrap();
            assert_eq!(allowed, case["result"].is_null(), "{case}");
        }
    }

    #[test]
    fn source_news_pattern_is_a_case_sensitive_substring_not_a_url_parser() {
        for comment in [
            "https://example.test",
            "http://",
            "xhttps://",
            "<a href=\"http://x\">x</a>",
        ] {
            assert_eq!(matches("@https?://@", comment), Ok(true));
        }
        for comment in [
            "",
            "0",
            "HTTP://example.test",
            "ftp://example.test",
            "https&#58;//x",
            "https:/ /x",
        ] {
            assert_eq!(matches("@https?://@", comment), Ok(false));
        }
    }

    #[test]
    fn php_delimiters_modifiers_and_disabled_rules() {
        assert_eq!(matches("", "body"), Ok(true));
        assert_eq!(matches("0", "body"), Ok(true));
        assert_eq!(matches("@https?://@i", "HTTP://x"), Ok(true));
        assert_eq!(matches("{^body$}D", "body\n"), Ok(false));
        assert_eq!(matches("//", ""), Ok(true));
        assert!(matches("/broken", "body").is_err());
        assert!(matches("/[/", "body").is_err());
        assert!(matches(&"x".repeat(MAX_PATTERN_BYTES + 1), "body").is_err());
    }

    #[test]
    fn pathological_matching_is_bounded() {
        assert!(matches("/^(a+)+$/", &format!("{}!", "a".repeat(10000))).is_err());
    }
}
