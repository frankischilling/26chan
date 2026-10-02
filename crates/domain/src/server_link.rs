//! The source server links its own sites. Other URLs remain text for the
//! optional browser linker. Parsing never grants arbitrary HTML authority.
use std::borrow::Cow;

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
            if let Some(last) = path[..len].bytes().rposition(path_end_byte) {
                end += last + 1;
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

fn digits(input: &str) -> Option<(&str, usize)> {
    let len = input.bytes().take_while(u8::is_ascii_digit).count();
    (len > 0).then_some((&input[..len], len))
}

fn normalized(input: &str, current: &str) -> Option<(String, usize)> {
    let (subdomain, domain, host_end) = host(input)?;
    if !subdomain.eq_ignore_ascii_case("boards") || domain == "4cdn.org" {
        return None;
    }
    // The source callback compares this spelling case sensitively.
    if subdomain != "boards" {
        return None;
    }
    let rest = input.get(host_end..)?.strip_prefix('/')?;
    let board_end = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    let board = rest.get(..board_end)?.to_ascii_lowercase();
    crate::BoardSlug::parse(&board).ok()?;
    let tail = rest.get(board_end..)?.strip_prefix('/')?;
    let start = input.len() - tail.len();
    let mut end = 0;
    let mut number = "";
    let mut catalog = false;
    if let Some(path) = prefix(tail, "thread/").or_else(|| prefix(tail, "res/")) {
        let (id, len) = digits(path)?;
        end = tail.len() - path.len() + len;
        number = id;
        if tail.as_bytes().get(end) == Some(&b'/') {
            let len = tail[end + 1..]
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
                .count();
            if len > 0 {
                end += len + 1;
            }
        }
        if tail.as_bytes().get(end) == Some(&b'#') {
            end += 1;
            if matches!(tail.as_bytes().get(end), Some(b'p' | b'P' | b'q' | b'Q')) {
                end += 1;
            }
            if let Some((id, len)) = digits(&tail[end..]) {
                number = id;
                end += len;
            }
        }
    } else if prefix(tail, "catalog").is_some() {
        end = 7;
        number = "catalog";
        catalog = true;
        if prefix(&tail[end..], "#s=").is_some() {
            let term = &tail[end + 3..];
            let len = term
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'+')
                .count();
            if len > 0 {
                number = &term[..len];
                end += 3 + len;
            }
        }
    } else {
        let name = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if name > 0
            && let Some(query) = prefix(&tail[name..], ".php?res=")
        {
            let (id, len) = digits(query)?;
            end = tail.len() - query.len() + len;
            number = id;
            if tail.as_bytes().get(end) == Some(&b'#') {
                end += 1;
                if matches!(tail.as_bytes().get(end), Some(b'p' | b'P' | b'q' | b'Q')) {
                    end += 1;
                }
                if let Some((id, len)) = digits(&tail[end..]) {
                    number = id;
                    end += len;
                }
            }
        }
    }
    if let Some(ch) = tail[end..].chars().next()
        && !ch.is_whitespace()
        && !".<!?,".contains(ch)
    {
        return None;
    }
    Some((
        if !catalog && !number.is_empty() && board == current {
            format!(">>{number}")
        } else {
            format!(">>>/{board}/{number}")
        },
        start + end,
    ))
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

    #[test]
    fn source_normalization_and_static_links_match_the_original_functions() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/format-reference.json")).unwrap();
        for case in reference["link_cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                normalize(input, "g"),
                case["normalized"].as_str().unwrap(),
                "{input}"
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
            assert_eq!(serde_json::json!(links), case["links"], "{input}");
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
