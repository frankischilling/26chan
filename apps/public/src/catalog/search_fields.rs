use board_domain::{Line, Token, parse_comment};

// Public posting currently bounds subjects to 120 bytes. The scalar cap also
// bounds this helper when invoked independently with oversized synthetic data.
const SUBJECT_SCALARS: usize = 120;

fn escape_into(output: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#039;"),
            _ => output.push(ch),
        }
    }
}

fn teaser(lines: &[Line]) -> String {
    let mut plain = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            plain.push(' ');
        }
        for token in &line.tokens {
            match token {
                Token::Text(text) | Token::Spoiler(text) | Token::Link(text) => {
                    plain.push_str(text);
                }
                Token::Quote(id) => {
                    plain.push_str(">>");
                    plain.push_str(&id.to_string());
                }
                Token::CrossQuote(board, id) => {
                    plain.push_str(">>>/");
                    plain.push_str(board);
                    plain.push('/');
                    plain.push_str(&id.to_string());
                }
            }
        }
    }
    // Collapse the ASCII whitespace represented by line breaks and normal
    // formatting. Do not infer normalization of unobserved Unicode spaces.
    let mut folded = String::new();
    let mut space = false;
    for ch in plain.chars() {
        if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}') {
            space = !folded.is_empty();
        } else {
            if space {
                folded.push(' ');
                space = false;
            }
            folded.push(ch);
        }
    }
    let mut escaped = String::new();
    escape_into(&mut escaped, &folded);
    escaped
}

pub(crate) fn from_parts(subject: &str, lines: &[Line]) -> String {
    let teaser = teaser(lines);
    if subject.is_empty() {
        return teaser;
    }
    let subject: String = subject.chars().take(SUBJECT_SCALARS).collect();
    let mut output = String::from("<b>");
    escape_into(&mut output, &subject);
    output.push_str("</b>");
    if !teaser.is_empty() {
        output.push_str(": ");
        output.push_str(&teaser);
    }
    output
}

pub(crate) fn from_raw(subject: &str, comment: &str) -> String {
    from_parts(subject, &parse_comment(comment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::filter::Filter;
    use proptest::prelude::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Contract {
        post_cases: Vec<PostCase>,
    }

    #[derive(Deserialize)]
    struct PostCase {
        name: String,
        subject: String,
        comment: String,
        text: String,
        checks: Vec<Check>,
    }

    #[derive(Deserialize)]
    struct Check {
        query: String,
        matches: bool,
    }

    #[test]
    fn shared_post_fields_and_queries_match_the_serialized_contract() {
        let contract: Contract = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/catalog-search-fields.json"
        ))
        .unwrap();
        for entry in contract.post_cases {
            let text = from_raw(&entry.subject, &entry.comment);
            assert_eq!(text, entry.text, "{}", entry.name);
            for check in entry.checks {
                assert_eq!(
                    Filter::new(&check.query).matches(&text),
                    check.matches,
                    "{}: {}",
                    entry.name,
                    check.query
                );
            }
        }
    }

    #[test]
    fn oversized_raw_inputs_remain_bounded_without_splitting_unicode() {
        let text = from_raw(&"'".repeat(10_000), &"&".repeat(20_000));
        assert_eq!(
            text.len(),
            7 + SUBJECT_SCALARS * 6 + 2 + board_domain::MAX_COMMENT_CHARS * 5
        );
        let unicode = from_raw("", &"\u{1f600}".repeat(20_000));
        assert_eq!(unicode.chars().count(), board_domain::MAX_COMMENT_CHARS);
    }

    proptest! {
        #[test]
        fn arbitrary_literal_text_cannot_supply_html_markup(input in ".{0,1024}") {
            let mut escaped = String::new();
            escape_into(&mut escaped, &input);
            prop_assert!(!escaped.contains('<'));
            prop_assert!(!escaped.contains('>'));
            prop_assert!(!escaped.contains('"'));
            prop_assert!(!escaped.contains('\''));
            prop_assert!(escaped.len() <= input.len() * 6);
        }

        #[test]
        fn raw_and_already_parsed_fields_have_one_representation(
            subject in ".{0,160}", comment in ".{0,1024}",
        ) {
            let parsed = parse_comment(&comment);
            prop_assert_eq!(from_raw(&subject, &comment), from_parts(&subject, &parsed));
        }
    }
}
