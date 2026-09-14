//! Source wordwrap2/utf8_wordwrap: ASCII spaces separate words; every 35
//! Unicode scalars get a soft-break marker, including an exact final group.
//! Generated HTML boundaries reset the count and link destinations stay intact.
use crate::formatting::{Token, tokenize_with};

const MARKER: &str = "{{w_br}}";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WordPart {
    Text(String),
    Break,
}

pub(crate) fn enabled(format: i16) -> bool {
    matches!(format, 40..=47 | 56..=63)
}

fn wrap(text: &str) -> String {
    let mut output = String::with_capacity(text.len() + text.len() / 35 * MARKER.len());
    let mut count = 0;
    for ch in text.chars() {
        output.push(ch);
        if ch == ' ' || ch == '\n' {
            count = 0;
        } else {
            count += 1;
            if count == 35 {
                output.push_str(MARKER);
                count = 0;
            }
        }
    }
    output
}

fn parts(text: &str) -> Vec<WordPart> {
    let mut output = Vec::new();
    for (index, text) in text.split(MARKER).enumerate() {
        if index > 0 {
            output.push(WordPart::Break);
        }
        if !text.is_empty() {
            output.push(WordPart::Text(text.into()));
        }
    }
    output
}

pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut output = Vec::new();
    // Link generation precedes word wrapping in the source. Quote-number
    // matching follows it, so a break can terminate a run of quote digits.
    for token in tokenize_with(text, false, false, true) {
        match token {
            Token::Link(url) => {
                let label = parts(&wrap(&url));
                output.push(Token::WrappedLink(url, label));
            }
            Token::Text(text) => {
                for token in tokenize_with(&wrap(&text), false, true, false) {
                    if let Token::Text(text) = token {
                        output.extend(parts(&text).into_iter().map(|part| match part {
                            WordPart::Text(text) => Token::Text(text),
                            WordPart::Break => Token::WordBreak,
                        }));
                    } else {
                        output.push(token);
                    }
                }
            }
            _ => unreachable!("link-only tokenization"),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_text_retains_scalars_and_has_bounded_break_runs(text in ".{0,16000}") {
            prop_assume!(!text.contains(MARKER));
            let wrapped=parts(&wrap(&text));
            let restored:String=wrapped.iter().filter_map(|p|match p {WordPart::Text(text)=>Some(text.as_str()),WordPart::Break=>None}).collect();
            prop_assert_eq!(&restored,&text);
            for part in wrapped {
                if let WordPart::Text(text)=part {
                    for run in text.split([' ','\n']) { prop_assert!(run.chars().count()<=35); }
                }
            }
        }
    }

    #[test]
    fn source_boundaries_use_scalars_ascii_spaces_and_trailing_breaks() {
        for count in [0, 1, 34, 35, 36, 69, 70, 71] {
            for ch in ['x', '界', '😀', '\u{a0}'] {
                let input = ch.to_string().repeat(count);
                assert_eq!(
                    parts(&wrap(&input))
                        .iter()
                        .filter(|p| matches!(p, WordPart::Break))
                        .count(),
                    count / 35
                );
            }
        }
        assert_eq!(
            wrap(&format!("{} {}", "x".repeat(34), "y".repeat(34))),
            format!("{} {}", "x".repeat(34), "y".repeat(34))
        );
        assert_eq!(
            wrap(&format!("{}\n{}", "x".repeat(34), "y".repeat(35))),
            format!("{}\n{}{{{{w_br}}}}", "x".repeat(34), "y".repeat(35))
        );
        assert_eq!(
            parts("left{{w_br}}right"),
            vec![
                WordPart::Text("left".into()),
                WordPart::Break,
                WordPart::Text("right".into())
            ]
        );
        assert_eq!(
            parts(&wrap(&format!("{}{{{{w_br}}}}", "x".repeat(32)))),
            vec![
                WordPart::Text(format!("{}{{{{w", "x".repeat(32))),
                WordPart::Break,
                WordPart::Text("_br}}".into())
            ]
        );
    }

    #[test]
    fn links_keep_their_destination_while_quote_digits_follow_the_wrapped_text() {
        let url = format!("https://example.com/{}", "x".repeat(100));
        let tokens = tokenize(&url);
        let Token::WrappedLink(href, label) = &tokens[0] else {
            panic!("wrapped link")
        };
        assert_eq!(href, &url);
        assert_eq!(
            label
                .iter()
                .filter(|p| matches!(p, WordPart::Break))
                .count(),
            3
        );
        let tokens = tokenize(&format!("{}>>1234567890", "x".repeat(30)));
        assert!(tokens.contains(&Token::Quote(123)));
        assert!(tokens.contains(&Token::Text("4567890".into())));
    }
}
