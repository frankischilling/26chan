use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Text(String),
    Quote(u64),
    Spoiler(String),
    Link(String),
}

#[derive(Clone, Debug)]
pub struct Line {
    pub green: bool,
    pub tokens: Vec<Token>,
}

/// Nonrecursive grammar. HTML is always text; templates escape every text node.
/// The posting boundary caps input at 16,000 Unicode scalar values. This parser
/// also bounds work when called independently on malformed or oversized input.
pub fn parse_comment(input: &str) -> Vec<Line> {
    let end = input
        .char_indices()
        .nth(crate::MAX_COMMENT_CHARS)
        .map_or(input.len(), |(index, _)| index);
    input[..end]
        .split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            Line {
                green: line.starts_with('>') && !line.starts_with(">>"),
                tokens: tokenize(line),
            }
        })
        .collect()
}

fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut text = String::new();
    let mut tail = line;
    while !tail.is_empty() {
        let mut found = None;
        if let Some(rest) = tail.strip_prefix("[spoiler]") {
            if let Some(end) = rest.find("[/spoiler]") {
                found = Some((Token::Spoiler(rest[..end].to_owned()), 9 + end + 10));
            }
        } else if let Some(rest) = tail.strip_prefix(">>") {
            let len = rest.bytes().take_while(u8::is_ascii_digit).count();
            if len > 0
                && len <= 19
                && let Ok(id) = rest[..len].parse::<u64>()
                && id > 0
                && id <= i64::MAX as u64
            {
                found = Some((Token::Quote(id), len + 2));
            }
        } else if tail.starts_with("https://") || tail.starts_with("http://") {
            let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
            let candidate = &tail[..end];
            if let Ok(url) = Url::parse(candidate)
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
            {
                found = Some((Token::Link(url.to_string()), end));
            }
        }
        if let Some((token, len)) = found {
            if !text.is_empty() {
                tokens.push(Token::Text(std::mem::take(&mut text)));
            }
            tokens.push(token);
            tail = &tail[len..];
        } else {
            let c = tail.chars().next().expect("nonempty tail");
            text.push(c);
            tail = &tail[c.len_utf8()..];
        }
    }
    if !text.is_empty() {
        tokens.push(Token::Text(text));
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_unicode_is_bounded_and_links_have_approved_schemes(input in ".{0,20000}") {
            let lines = parse_comment(&input);
            prop_assert!(lines.len() <= 16001);
            let count: usize = lines.iter().map(|line| line.tokens.len()).sum();
            prop_assert!(count <= 16000);
            for line in lines { for token in line.tokens {
                if let Token::Link(link) = token {
                    let url = Url::parse(&link).unwrap();
                    prop_assert!(matches!(url.scheme(), "http" | "https"));
                    prop_assert!(url.username().is_empty() && url.password().is_none());
                }
            } }
        }

        #[test]
        fn scalar_limit_is_preserved_for_multibyte_and_combining_text(
            chars in prop::collection::vec(
                prop_oneof![Just('é'), Just('😀'), Just('e'), Just('\u{301}')],
                1..20_001,
            ),
        ) {
            let input: String = chars.into_iter().collect();
            let expected: String = input.chars().take(crate::MAX_COMMENT_CHARS).collect();
            let lines = parse_comment(&input);
            prop_assert_eq!(lines.len(), 1);
            prop_assert_eq!(&lines[0].tokens, &vec![Token::Text(expected)]);
        }
    }

    #[test]
    fn parser_preserves_a_full_unicode_comment_and_truncates_by_scalar() {
        let accepted = "😀".repeat(16_000);
        let lines = parse_comment(&accepted);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tokens, vec![Token::Text(accepted)]);

        let oversized = "😀".repeat(16_001);
        let lines = parse_comment(&oversized);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].tokens, vec![Token::Text("😀".repeat(16_000))]);
    }
}
