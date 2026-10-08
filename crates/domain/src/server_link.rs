//! The source server links its own sites. Other URLs remain text for the
//! optional browser linker. Parsing never grants arbitrary HTML authority.
use pcre2::bytes::Regex;
use std::{borrow::Cow, sync::OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerLink(String);

impl ServerLink {
    pub fn href(&self) -> &str {
        &self.0
    }

    pub(crate) fn without_source_markers(mut self) -> Self {
        self.0 = crate::filtered_formatting::remove_source_markers(&self.0);
        self
    }

    pub(crate) fn parse(input: &str) -> Option<(Self, usize)> {
        let (_, _, host_end) = host(input)?;
        let mut end = host_end;
        if input.as_bytes().get(end) == Some(&b'/') {
            let path = &input[end..];
            let len = path.bytes().take_while(|byte| path_byte(*byte)).count();
            // The source requires a path character after the first slash.
            // A bare trailing slash stays outside the anchor.
            if len > 1
                && let Some(last) = path[1..len].bytes().rposition(path_end_byte)
            {
                end += last + 2;
            }
        }
        let candidate = &input[..end];
        let parsed = url::Url::parse(candidate).ok()?;
        if !parsed.has_host() || !parsed.username().is_empty() || parsed.password().is_some() {
            return None;
        }
        Some((Self(candidate.into()), end))
    }
}

fn prefix<'a>(input: &'a str, expected: &str) -> Option<&'a str> {
    input
        .get(..expected.len())?
        .eq_ignore_ascii_case(expected)
        .then(|| &input[expected.len()..])
}

fn host(input: &str) -> Option<(&str, &str, usize)> {
    let rest = prefix(input, "https://").or_else(|| prefix(input, "http://"))?;
    let start = input.len() - rest.len();
    // Like the source regex, subdomains contain letters only. An unrecognized
    // port, suffix or credential is left outside the matched link.
    let sublen = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (subdomain, base) = if rest.as_bytes().get(sublen) == Some(&b'.') {
        (&rest[..sublen], &rest[sublen + 1..])
    } else {
        ("", rest)
    };
    let domain = ["4chan.org", "4channel.org", "4cdn.org"]
        .into_iter()
        .find(|domain| prefix(base, domain).is_some())?;
    Some((
        subdomain,
        domain,
        start + rest.len() - base.len() + domain.len(),
    ))
}

fn path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_-.,@?^=%&;:/~+#()".contains(&byte)
}

fn path_end_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_-@?^=%&;/~+#".contains(&byte)
}

pub(crate) fn source_probe(input: &str) -> bool {
    input.contains("4chan") || input.contains("4cdn.org")
}

pub(crate) fn link_probe(input: &str) -> bool {
    (input.contains("4chan") && !input.contains("/derefer")) || input.contains("4cdn.org")
}

fn normalized(input: &str, current: &str) -> Option<(String, usize)> {
    prefix(input, "https://").or_else(|| prefix(input, "http://"))?;
    // Keep the source's byte-mode captures and backtracking. In particular,
    // .php has a wildcard dot, and \s does not include Unicode whitespace.
    // This fixed expression produces quote text, never caller-supplied HTML.
    static NORMALIZE: OnceLock<Regex> = OnceLock::new();
    let regex = NORMALIZE.get_or_init(|| {
        Regex::new(r"(?i)\Ahttps?://([a-z]*)[.](?:4chan|4channel)[.]org/(\w+)/(?:(res|thread)/(\d+)(?:/[-a-z0-9]+)?(?:#[qp]?(\d*))?|(catalog(?:#s=[a-z0-9+]+)?)|\w+.php[?]res=(\d+)(?:#[qp]?(\d*))?|)(?=[\s.<!?,]|$)")
            .expect("fixed source normalization expression")
    });
    let captures = regex.captures(input.as_bytes()).ok()??;
    let matched = captures.get(0)?;
    // Callers pass plain text, while the source runs after entity escaping.
    // The wildcard cannot consume one of these expanded source entities, and
    // a literal user '<' becomes '&lt;', not the source's HTML delimiter.
    if matched
        .as_bytes()
        .iter()
        .any(|byte| b"&<>\"'".contains(byte))
        || input.as_bytes().get(matched.end()) == Some(&b'<')
    {
        return None;
    }
    let capture = |index| {
        captures.get(index).map_or("", |value| {
            std::str::from_utf8(value.as_bytes()).expect("ASCII source capture")
        })
    };
    // The regex is case-insensitive, but the callback's comparison is not.
    if capture(1) != "boards" {
        return None;
    }
    // The source accepts ASCII word characters without BoardSlug's routing
    // length/reservation restrictions. This result remains ordinary text.
    let board = capture(2).to_ascii_lowercase();
    // PHP scans backwards for a truthy capture: exactly "0" is false, "00"
    // is true. With thread/0 this can select the preceding "thread" capture;
    // a root URL has no truthy capture and therefore an empty number.
    let number = (3..=8)
        .rev()
        .map(capture)
        .find(|value| !value.is_empty() && *value != "0")
        .unwrap_or("");
    let quote = if number
        .get(..7)
        .is_some_and(|value| value.eq_ignore_ascii_case("catalog"))
    {
        let term = number
            .to_ascii_lowercase()
            .find("#s=")
            .map_or("catalog", |index| &number[index + 3..]);
        format!(">>>/{board}/{term}")
    } else if board == current && !number.is_empty() && number != "catalog" {
        format!(">>{number}")
    } else {
        format!(">>>/{board}/{number}")
    };
    Some((quote, matched.end()))
}

pub(crate) fn normalize<'a>(input: &'a str, current: &str) -> Cow<'a, str> {
    normalize_with_probe(input, current, source_probe(input))
}

pub(crate) fn normalize_with_probe<'a>(
    input: &'a str,
    current: &str,
    enabled: bool,
) -> Cow<'a, str> {
    if !enabled {
        return Cow::Borrowed(input);
    }
    let mut output = String::with_capacity(input.len());
    let mut tail = input;
    while !tail.is_empty() {
        if let Some((quote, len)) = normalized(tail, current) {
            output.push_str(&quote);
            tail = &tail[len..];
        } else {
            let ch = tail.chars().next().expect("nonempty input");
            output.push(ch);
            tail = &tail[ch.len_utf8()..];
        }
    }
    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Token, parse_post_comment_on_board, word_break::WordPart};
    use proptest::prelude::*;

    fn assert_source_case(case: &serde_json::Value) {
        let input = case["input"].as_str().unwrap();
        assert_eq!(
            normalize(input, "g"),
            case["normalized"].as_str().unwrap(),
            "normalization: {input:?}"
        );
        let mut links = Vec::new();
        for line in parse_post_comment_on_board(input, 104, "g") {
            for token in line.tokens {
                match token {
                    Token::StaticQuote(quote, _) => links.push(serde_json::json!({
                        "href": quote.href(), "label": quote.label(), "new_tab": quote.opens_new_tab()})),
                    Token::ServerLink(link, parts) => {
                        let label: String = parts.iter().filter_map(|part| match part {
                            WordPart::Text(text) => Some(text.as_str()), WordPart::Break => None
                        }).collect();
                        links.push(serde_json::json!({"href": link.href(), "label": label, "new_tab": true}));
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(serde_json::json!(links), case["links"], "links: {input:?}");
    }

    #[test]
    fn source_normalization_and_static_links_match_the_original_functions() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/format-reference.json")).unwrap();
        for case in reference["link_cases"].as_array().unwrap() {
            assert_source_case(case);
        }
    }

    #[test]
    fn source_byte_boundaries_truthy_captures_and_server_roots_match_php() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/source-link-boundary-reference.json"
        ))
        .unwrap();
        for case in reference["escaped_normalization_cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                normalize(input, "g"),
                case["normalized"].as_str().unwrap(),
                "{input:?}"
            );
        }
        for category in ["normalization_cases", "static_cases", "server_cases"] {
            for case in reference[category].as_array().unwrap() {
                assert_source_case(case);
            }
        }
    }

    #[test]
    fn disabling_the_source_probe_leaves_normalization_untouched() {
        for input in [
            "https://boards.4chan.org/g/thread/0",
            "https://boards.4chan.org/g/imgboardXphp?res=42",
            "https://boards.4chan.org/longboard12345/",
        ] {
            assert!(matches!(
                normalize_with_probe(input, "g", false),
                Cow::Borrowed(_)
            ));
            assert_eq!(normalize_with_probe(input, "g", false), input);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_input_cannot_grant_unapproved_link_authority(input in ".{0,1024}") {
            for line in parse_post_comment_on_board(&input, 104, "g") {
                for token in line.tokens {
                    prop_assert!(!matches!(token, Token::Link(_) | Token::WrappedLink(_, _)));
                    if let Token::ServerLink(link, _) = token {
                        let parsed = url::Url::parse(link.href()).unwrap();
                        let host = parsed.host_str().unwrap();
                        prop_assert!(host == "4chan.org" || host == "4channel.org" || host == "4cdn.org"
                            || host.ends_with(".4chan.org") || host.ends_with(".4channel.org") || host.ends_with(".4cdn.org"));
                        prop_assert!(parsed.username().is_empty() && parsed.password().is_none());
                        prop_assert!(matches!(parsed.scheme(), "http" | "https"));
                    }
                }
            }
        }
    }
}
