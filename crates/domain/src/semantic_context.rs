//! Source thread-link context (`generate_href_context`, imgboard.php 7938).
//! Inputs are escaped subject and source-shaped formatted comment data, never
//! trusted HTML. The result is a text label, not markup or a routing decision.
use pcre2::bytes::Regex;
use std::sync::LazyLock;

// Fixed patterns still need finite work limits for pathological historical
// comment data. Errors propagate as a failed projection, never partial output.
fn pattern(source: &str) -> Regex {
    Regex::new(&format!(
        "(*LIMIT_MATCH=1000000)(*LIMIT_DEPTH=64)(*LIMIT_HEAP=1024){source}"
    ))
    .expect("fixed source context pattern")
}

static URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(^|\s)https?://[^\s]{4,}"));
static ABBREVIATION: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"<span class="abbr">.*</table>"#));
static STRONG: LazyLock<Regex> = LazyLock::new(|| pattern(r"<strong [^>]+>.*</strong>"));
static TAG: LazyLock<Regex> = LazyLock::new(|| pattern(r"<[^>]+>"));

/// Mirrors source ordering, including first-line selection before tag removal.
/// A private janitor board never exposes a context. Callers omit empty results.
pub fn generate(subject: &str, comment: &str, janitor_board: bool) -> Result<String, pcre2::Error> {
    if janitor_board {
        return Ok(String::new());
    }
    // Only the source's internal, unescaped marker is stripped. A literal
    // submitted subject contains SPOILER&lt;&gt; after source escaping.
    let subject = subject.strip_prefix("SPOILER<>").unwrap_or(subject);
    let context = cleanup(subject);
    if !context.is_empty() || comment.is_empty() {
        return Ok(context);
    }
    let has_br = comment.contains("<br>");
    let mut context = comment.replace("<br>", "\n");
    context = replace(&URL, &context, "")?;
    if context.contains(r#"<span class="abbr">"#) {
        context = replace(&ABBREVIATION, &context, "")?;
    }
    if context.contains("<strong") {
        context = replace(&STRONG, &context, "")?;
    }
    if has_br {
        // PHP ltrim's default differs from both Unicode whitespace and \s.
        context = context
            .trim_start_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
            .split('\n')
            .next()
            .unwrap_or_default()
            .to_owned();
    }
    Ok(cleanup(&replace(&TAG, &context, " ")?))
}

fn replace(regex: &Regex, input: &str, replacement: &str) -> Result<String, pcre2::Error> {
    let mut result = String::new();
    let mut cursor = 0;
    for matched in regex.find_iter(input.as_bytes()) {
        let matched = matched?;
        // These fixed byte-mode patterns start/end at ASCII delimiters, so
        // they cannot split a UTF-8 scalar even though source counts bytes.
        result.push_str(&input[cursor..matched.start()]);
        result.push_str(replacement);
        cursor = matched.end();
    }
    result.push_str(&input[cursor..]);
    Ok(result)
}

fn cleanup(input: &str) -> String {
    let decoded = decode_special_entities(input);
    let normalized: String = decoded
        .bytes()
        .filter(|byte| {
            byte.is_ascii_alphanumeric() || *byte == b' ' || (b'\t'..=b'\r').contains(byte)
        })
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect();
    let mut length = 0;
    normalized
        .split(' ')
        .filter(|word| !word.is_empty())
        .take_while(|word| {
            length += word.len() + 1;
            length <= 50
        })
        .collect::<Vec<_>>()
        .join("-")
}

// htmlspecialchars_decode(ENT_QUOTES | ENT_HTML401), not html_entity_decode:
// leave other named/numeric entities as literal text and never decode twice.
pub(crate) fn decode_special_entities(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest[1..]
            .find([';', '&'])
            .map(|end| end + 1)
            .filter(|end| rest.as_bytes()[*end] == b';')
            .and_then(|end| {
                let entity = &rest[1..end];
                let scalar = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    _ => entity.strip_prefix('#').and_then(|number| {
                        let (digits, radix) = number
                            .strip_prefix(['x', 'X'])
                            .map_or((number, 10), |hex| (hex, 16));
                        if digits.is_empty()
                            || !digits.bytes().all(|byte| {
                                if radix == 16 {
                                    byte.is_ascii_hexdigit()
                                } else {
                                    byte.is_ascii_digit()
                                }
                            })
                        {
                            return None;
                        }
                        u32::from_str_radix(digits.trim_start_matches('0'), radix)
                            .ok()
                            .and_then(char::from_u32)
                            .filter(|ch| matches!(ch, '&' | '<' | '>' | '"' | '\''))
                    }),
                };
                scalar.map(|ch| (end + 1, ch))
            });
        if let Some((length, ch)) = decoded {
            output.push(ch);
            rest = &rest[length..];
        } else {
            output.push('&');
            rest = &rest[1..];
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_cleanup_keeps_whole_words_and_removes_punctuation() {
        for (input, expected) in [
            ("What are you making?", "what-are-you-making"),
            ("foo-bar don't foo/bar foo_bar", "foobar-dont-foobar-foobar"),
            ("café 日本語 A&B", "caf-ab"),
            ("a  b\tc\nd", "a-b\tc\nd"),
            ("&lt;tag&gt; &quot;a&quot; &#039;b&#039; &amp;", "tag-a-b"),
            ("&amp;lt; &nbsp; &#65; &apos;", "lt-nbsp-65-apos"),
            ("&#60;x&#x3e; &#34;q&#00039;", "x-q"),
        ] {
            assert_eq!(cleanup(input), expected, "{input}");
        }
        assert_eq!(cleanup(&"a".repeat(49)), "a".repeat(49));
        assert_eq!(cleanup(&"a".repeat(50)), "");
        assert_eq!(
            cleanup(&format!("{} b", "a".repeat(47))),
            format!("{}-b", "a".repeat(47))
        );
        assert_eq!(cleanup(&format!("{} bb c", "a".repeat(47))), "a".repeat(47));
    }

    #[test]
    fn malformed_and_unicode_inputs_remain_bounded_text_projections() {
        // Exercise the maximum-sized malformed delimiter run rather than
        // assuming a closing tag exists. Either a complete result or the
        // regex work-limit error is acceptable; never return partial context.
        for text in [
            "<".repeat(131_072),
            format!("{}>", "<".repeat(131_072)),
            format!("{}x</strong>", "<strong ".repeat(16_384)),
            "&".repeat(131_072),
        ] {
            if let Ok(context) = generate("", &text, false) {
                assert!(context.len() <= 49);
                assert!(!context.contains(['<', '>', '&']));
            }
        }
        // PCRE runs in the source's byte mode: four UTF-8 bytes qualify for
        // the URL minimum, while two bytes do not. Endpoints stay boundaries.
        assert_eq!(generate("", "http://éé after", false).unwrap(), "after");
        assert_eq!(generate("", "http://é after", false).unwrap(), "http-after");
        assert_eq!(generate("", "<s>日本語</s> café", false).unwrap(), "caf");
        assert_eq!(
            decode_special_entities("&amp;lt; &#0000000000000000000039; &apos;"),
            "&lt; ' &apos;"
        );
    }

    #[test]
    fn context_order_matches_source_formatted_comment_branches() {
        for (subject, comment, expected) in [
            ("Preferred!", "fallback", "preferred"),
            ("!!!", "fallback", "fallback"),
            ("SPOILER<>Hidden title", "fallback", "hidden-title"),
            ("SPOILER&lt;&gt;Literal", "", "spoilerliteral"),
            ("", "first<br>second", "first"),
            ("", "<br>  first<br>second", "first"),
            ("", "<s></s><br>second", ""),
            ("", "first\nsecond", "first\nsecond"),
            ("", "<b>Rolled 3 (1d6)<br><br></b>body", "rolled-3-1d6"),
            (
                "",
                "!!!<span class=\"fortune\" style=\"color:red\"><br><br><b>Your fortune: Good Luck</b></span>",
                "",
            ),
            ("", "foo<s>bar</s>baz", "foo-bar-baz"),
            ("", "&lt;s&gt;literal&lt;/s&gt;", "sliterals"),
            ("", "https://example.test<br>after url", "after-url"),
            ("", "before https://example.test after", "before-after"),
            ("", "http://abc stays", "httpabc-stays"),
            ("", "HTTPS://example.test stays", "httpsexampletest-stays"),
            (
                "",
                "<a href=\"https://example.test\">https://example.test</a>",
                "httpsexampletest",
            ),
            (
                "",
                "before <strong class=\"x\">hidden</strong> after",
                "before-after",
            ),
            ("", "<strong>kept</strong>", "kept"),
            (
                "",
                "before <span class=\"abbr\">hidden</table> after",
                "before-after",
            ),
        ] {
            assert_eq!(
                generate(subject, comment, false).unwrap(),
                expected,
                "{subject:?} {comment:?}"
            );
        }
        assert_eq!(
            generate(&"a".repeat(50), "fallback", false).unwrap(),
            "fallback"
        );
        assert_eq!(generate("title", "comment", true).unwrap(), "");
    }
}
