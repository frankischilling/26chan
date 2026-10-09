//! Thread-title context from `generate_page_title` (imgboard.php 7990).
//! Inputs are source-escaped subjects and source-shaped comment data. The
//! result is decoded text and must be escaped at every HTML output boundary.
use crate::semantic_context::decode_special_entities;
use pcre2::bytes::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

// Covers the maximum admitted, escaped staff subject plus internal markers.
const MAX_SUBJECT_DATA_BYTES: usize = 8192;
static SJIS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(*LIMIT_MATCH=1000000)(*LIMIT_DEPTH=64)(*LIMIT_HEAP=1024)<span class="sjis".+?</span>"#,
    )
    .expect("fixed page-title SJIS pattern")
});

/// Return the source title context without the board prefix or No.id fallback.
/// Subjects win even when they contain only whitespace and are not shortened.
/// Oversized or failed projections return empty so callers can use No.id.
pub fn context(subject: &str, comment: &str, upload_board: bool, sjis: bool) -> String {
    if subject.len() > MAX_SUBJECT_DATA_BYTES {
        return String::new();
    }
    let subject = if upload_board {
        let digits = subject.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && subject.as_bytes().get(digits) == Some(&b'|') {
            &subject[digits + 1..]
        } else {
            subject
        }
    } else {
        subject
    };
    let subject = subject.strip_prefix("SPOILER<>").unwrap_or(subject);
    if !subject.is_empty() {
        return decode_special_entities(subject);
    }
    if comment.is_empty()
        || comment.len() > crate::WordfilterLimits::Authorized.saved_post_read_bytes()
    {
        return String::new();
    }
    let comment = if sjis && comment.contains(r#"<span class="sjis""#) {
        let Some(replaced) = replace_sjis(comment) else {
            return String::new();
        };
        Cow::Owned(replaced)
    } else {
        Cow::Borrowed(comment)
    };
    let decoded = decode_special_entities(&comment.replace("<br>", " "));
    strip_tags(&decoded).chars().take(50).collect()
}

fn replace_sjis(input: &str) -> Option<String> {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for found in SJIS.find_iter(input.as_bytes()) {
        let found = found.ok()?;
        // This byte-mode pattern has ASCII endpoints; UTF-8 scalars stay whole.
        output.push_str(&input[cursor..found.start()]);
        output.push_str("[SJIS]");
        cursor = found.end();
    }
    output.push_str(&input[cursor..]);
    Some(output)
}

#[derive(Clone, Copy)]
enum TagState {
    Text,
    Html,
    Php,
    Declaration,
    Comment,
}

// Scanner states follow PHP 8.3/8.4 php_strip_tags_ex without allowed tags:
// https://github.com/php/php-src/blob/PHP-8.3.6/ext/standard/string.c
// The captured application helper is qualified separately by the PHP oracle.
// Quotes can contain '>', tags do not add spaces, and '< ' remains text.
// This finite-state scan
// handles historical malformed data without trusting it or using a tag regex.
fn strip_tags(input: &str) -> String {
    use TagState::*;
    let bytes = input.as_bytes();
    let mut state = Text;
    let mut quote = '\0';
    let mut significant = '\0';
    let mut depth = 0usize;
    let mut brackets = 0isize;
    let mut xml = false;
    let mut output = String::with_capacity(input.len());
    for (index, ch) in input.char_indices() {
        let previous = index.checked_sub(1).map(|i| bytes[i]);
        let next_space = bytes
            .get(index + 1)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'..=b'\r'));
        match state {
            Text => match ch {
                '\0' => {}
                '<' if quote != '\0' => {}
                '<' if !next_space => {
                    significant = '<';
                    state = Html;
                }
                '>' if depth > 0 => depth -= 1,
                '>' if quote != '\0' => {}
                _ => output.push(ch),
            },
            Html => match ch {
                '<' if quote == '\0' && !next_space => depth += 1,
                '>' if depth > 0 => depth -= 1,
                '>' if quote == '\0' => {
                    significant = '>';
                    if !xml || previous != Some(b'-') {
                        state = Text;
                        xml = false;
                    }
                }
                '\'' | '"' => toggle_quote(&mut quote, ch),
                '!' if previous == Some(b'<') => {
                    significant = '!';
                    state = Declaration;
                }
                '?' if previous == Some(b'<') => {
                    brackets = 0;
                    state = Php;
                }
                _ => {}
            },
            Php => match ch {
                '(' | ')' if !matches!(significant, '\'' | '"') => {
                    significant = ch;
                    brackets += if ch == '(' { 1 } else { -1 };
                }
                '>' if depth > 0 => depth -= 1,
                '>' if quote == '\0'
                    && brackets == 0
                    && significant != '"'
                    && previous == Some(b'?') =>
                {
                    state = Text;
                }
                '\'' | '"' if previous != Some(b'\\') => {
                    if significant == ch {
                        significant = '\0';
                    } else if significant != '\\' {
                        significant = ch;
                    }
                    toggle_quote(&mut quote, ch);
                }
                'l' | 'L' if index > 4 && bytes[index - 4..index].eq_ignore_ascii_case(b"<?xm") => {
                    state = Html;
                    xml = true;
                }
                _ => {}
            },
            Declaration => match ch {
                '>' if depth > 0 => depth -= 1,
                '>' if quote == '\0' => state = Text,
                '\'' | '"' if previous != Some(b'\\') => toggle_quote(&mut quote, ch),
                '-' if index >= 2 && &bytes[index - 2..index] == b"!-" => state = Comment,
                'e' | 'E'
                    if index > 6 && bytes[index - 6..index].eq_ignore_ascii_case(b"doctyp") =>
                {
                    state = Html;
                }
                _ => {}
            },
            Comment => {
                if ch == '>' && quote == '\0' && index >= 2 && &bytes[index - 2..index] == b"--" {
                    state = Text;
                }
            }
        }
    }
    output
}

fn toggle_quote(quote: &mut char, ch: char) {
    if *quote == ch {
        *quote = '\0';
    } else if *quote == '\0' {
        *quote = ch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_title_context_fixtures() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/page-title-reference.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 40);
        for case in cases {
            assert_eq!(
                context(
                    case["subject"].as_str().unwrap(),
                    case["comment"].as_str().unwrap(),
                    case["upload_board"].as_bool().unwrap(),
                    case["sjis"].as_bool().unwrap(),
                ),
                case["source"]["context"].as_str().unwrap(),
                "{}",
                case["id"]
            );
        }
    }

    #[test]
    fn limits_fail_closed_without_cropping_subjects() {
        let admitted = crate::source_html_entities(&"'".repeat(crate::MAX_AUTHORIZED_FIELD_BYTES));
        assert_eq!(context(&admitted, "ignored", false, false), "'".repeat(255));
        assert_eq!(
            context(
                &"x".repeat(MAX_SUBJECT_DATA_BYTES + 1),
                "comment",
                false,
                false
            ),
            ""
        );
        let oversized = "x".repeat(crate::WordfilterLimits::Authorized.saved_post_read_bytes() + 1);
        assert_eq!(context("", &oversized, false, true), "");
        assert_eq!(context("subject", &oversized, false, true), "subject");
        for hostile in [
            "<".repeat(131_072),
            "&".repeat(131_072),
            r#"<span class="sjis""#.repeat(8192),
            r#"<a title=""#.repeat(8192),
        ] {
            assert!(context("", &hostile, false, true).chars().count() <= 50);
        }
    }

    #[test]
    fn context_is_plain_text_never_a_trusted_html_type() {
        assert_eq!(
            context("&lt;script&gt;x&lt;/script&gt;", "", false, false),
            "<script>x</script>"
        );
        assert_eq!(
            context("", "&lt;script&gt;x&lt;/script&gt;", false, false),
            "x"
        );
        assert_eq!(
            context("", "&amp;lt;script&amp;gt;", false, false),
            "&lt;script&gt;"
        );
    }
}
