use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Text(String),
    Quote(u64),
    CrossQuote(String, u64),
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
        } else if let Some(rest) = tail.strip_prefix(">>>/") {
            if let Some(end) = rest.bytes().take(11).position(|byte| byte == b'/')
                && let Ok(board) = crate::BoardSlug::parse(&rest[..end])
                && let Some((id, digits)) = post_number(&rest[end + 1..])
            {
                let consumed = 4 + board.as_str().len() + 1 + digits;
                found = Some((Token::CrossQuote(board.as_str().into(), id), consumed));
            }
        } else if let Some(rest) = tail.strip_prefix(">>") {
            if let Some((id, len)) = post_number(rest) {
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

fn post_number(input: &str) -> Option<(u64, usize)> {
    let len = input
        .bytes()
        .take(20)
        .take_while(u8::is_ascii_digit)
        .count();
    if len == 0 || len > 19 {
        return None;
    }
    let id = input[..len].parse::<u64>().ok()?;
    (id > 0 && id <= i64::MAX as u64).then_some((id, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn cross_board_quotes_round_trip_to_bounded_local_identifiers(
            board in "[a-z0-9]{1,10}", id in 1u64..=i64::MAX as u64,
        ) {
            let lines = parse_comment(&format!(">>>/{board}/{id}"));
            prop_assert_eq!(&lines[0].tokens, &vec![Token::CrossQuote(board, id)]);
            prop_assert!(!lines[0].green);
        }

        #[test]
        fn arbitrary_cross_board_inputs_never_supply_url_paths(input in ".{0,512}") {
            for line in parse_comment(&format!(">>>/{input}")) {
                for token in line.tokens {
                    if let Token::CrossQuote(board, id) = token {
                        prop_assert!(crate::BoardSlug::parse(&board).is_ok());
                        prop_assert!(id > 0 && id <= i64::MAX as u64);
                        let base = Url::parse("https://board.example/").unwrap();
                        let link = base.join(&format!("/{board}/post/{id}")).unwrap();
                        prop_assert_eq!(link.origin(), base.origin());
                        prop_assert!(link.query().is_none() && link.fragment().is_none());
                    }
                }
            }
        }
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
    fn cross_board_quotes_preserve_surrounding_text_and_spoiler_boundaries() {
        let lines = parse_comment("See >>>/po/42, then >>43.\n[spoiler]>>>/po/42[/spoiler]");
        assert_eq!(
            lines[0].tokens,
            vec![
                Token::Text("See ".into()),
                Token::CrossQuote("po".into(), 42),
                Token::Text(", then ".into()),
                Token::Quote(43),
                Token::Text(".".into()),
            ]
        );
        assert_eq!(lines[1].tokens, vec![Token::Spoiler(">>>/po/42".into())]);
        for invalid in [
            ">>>//1",
            ">>>/../1",
            ">>>/PO/1",
            ">>>/é/1",
            ">>>/abcdefghijk/1",
            ">>>/%2f/1",
            ">>>/a\\b/1",
            ">>>/po/0",
            ">>>/po/-1",
            ">>>/po/9223372036854775808",
            ">>>/po/00000000000000000001",
            ">>>/po/",
            ">>>/po/١",
            ">>>/po\"/1",
        ] {
            assert_eq!(
                parse_comment(invalid)[0].tokens,
                vec![Token::Text(invalid.into())],
                "{invalid}"
            );
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
