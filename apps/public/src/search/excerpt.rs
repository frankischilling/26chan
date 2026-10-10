//! Clip the completed typed representation. Normalization, word wrapping and
//! quote eligibility have already run against the entire saved comment.
use board_domain::{Line, Token, formatting::visible_text as text};
use std::borrow::Cow;

fn close(open: &Token) -> Option<Token> {
    match open {
        Token::OpenMarkup(tag) => Some(Token::CloseMarkup(*tag)),
        Token::OpenQuote => Some(Token::CloseQuote),
        Token::FilteredDelimiter(delimiter) if delimiter.valid_element() && delimiter.opening() => {
            Some(Token::FilteredDelimiter(delimiter.closing()))
        }
        Token::GeneratedBold(true) => Some(Token::GeneratedBold(false)),
        Token::GeneratedSmall(true) => Some(Token::GeneratedSmall(false)),
        Token::GeneratedFortune(true, color) => Some(Token::GeneratedFortune(false, color.clone())),
        _ => None,
    }
}

pub(super) fn lines(input: &[Line], query: &str, maximum: usize) -> Vec<Line> {
    let maximum = maximum.min(1024);
    if maximum == 0 {
        return Vec::new();
    }
    let mut lower = String::new();
    let mut positions = Vec::new();
    let mut scalar: usize = 0;
    for (index, line) in input.iter().enumerate() {
        let values = (index > 0)
            .then_some(Cow::Borrowed("\n"))
            .into_iter()
            .chain(line.tokens.iter().map(text));
        for value in values {
            for ch in value.chars() {
                positions.push((lower.len(), scalar));
                lower.extend(ch.to_lowercase());
                scalar += 1;
            }
        }
    }
    // The HTTP search boundary allows at most 512 UTF-16 units. Retain a
    // separate cap here so this helper also has bounded query allocation.
    let needle: String = query
        .chars()
        .take(512)
        .flat_map(char::to_lowercase)
        .collect();
    let matched = lower.find(&needle).map_or(0, |byte| {
        let index = positions
            .partition_point(|(offset, _)| *offset <= byte)
            .saturating_sub(1);
        positions.get(index).map_or(0, |(_, scalar)| *scalar)
    });
    let start = matched.saturating_sub(maximum / 4);
    let end = start.saturating_add(maximum);
    let mut output: Vec<Line> = Vec::new();
    let mut active: Vec<(Token, Token)> = Vec::new();
    let mut cursor = 0;
    for (index, line) in input.iter().enumerate() {
        if index > 0 {
            cursor += 1;
        }
        if cursor >= end {
            break;
        }
        let mut target = if cursor >= start {
            let prefix = if output.is_empty() {
                active.iter().map(|(open, _)| open.clone()).collect()
            } else {
                Vec::new()
            };
            output.push(Line {
                green: line.green,
                tokens: prefix,
            });
            Some(output.len() - 1)
        } else {
            None
        };
        for token in &line.tokens {
            if cursor >= end {
                break;
            }
            let value = text(token);
            let count = value.chars().count();
            let begins = cursor >= start || cursor + count > start;
            if begins && target.is_none() {
                let prefix = if output.is_empty() {
                    active.iter().map(|(open, _)| open.clone()).collect()
                } else {
                    Vec::new()
                };
                output.push(Line {
                    green: line.green,
                    tokens: prefix,
                });
                target = Some(output.len() - 1);
            }
            if let Some(index) = target {
                if count == 0 || (cursor >= start && cursor + count <= end) {
                    output[index].tokens.push(token.clone());
                } else if cursor + count > start {
                    let skip = start.saturating_sub(cursor);
                    let take = end.saturating_sub(cursor + skip);
                    output[index]
                        .tokens
                        .push(Token::Text(value.chars().skip(skip).take(take).collect()));
                }
            }
            if let Some(closing) = close(token) {
                active.push((token.clone(), closing));
            } else if let Some(index) = active.iter().rposition(|(_, closing)| closing == token) {
                active.remove(index);
            }
            cursor += count;
        }
    }
    if let Some(last) = output.last_mut() {
        last.tokens
            .extend(active.into_iter().rev().map(|(_, close)| close));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::{
        comment_markup::MarkupPolicy,
        wordfilter::{LeetRolls, Profile},
    };
    use proptest::prelude::*;

    fn prepared(input: &str, profile: Profile, rolls: Option<LeetRolls>) -> Vec<Line> {
        let mut comment = board_domain::wordfiltered_comment::prepare(
            input,
            MarkupPolicy {
                code: true,
                spoilers: true,
                ..MarkupPolicy::default()
            },
            profile,
            rolls,
        )
        .unwrap();
        comment.freeze_format("g");
        board_domain::filtered_formatting::lines(&comment, "g")
    }
    fn projection(lines: &[Line]) -> String {
        board_domain::filtered_formatting::source_projection(lines)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn completed_unicode_excerpts_stay_within_the_requested_visible_limit(
            text in ".{0,512}", first in 0u8..6, second in 0u8..6,
        ) {
            let original=prepared(&text,Profile::Test,Some(LeetRolls::from_choices(first,second).unwrap()));
            for maximum in [0,1,7,32,128,1024] {
                let clipped=lines(&original,"owned",maximum);
                prop_assert!(board_domain::formatting::plain_text(&clipped).chars().count()<=maximum);
            }
        }
    }

    #[test]
    fn late_matches_use_normalized_links_and_removed_source_markers() {
        let original = prepared(
            &format!(
                "{} [code]{} https://boards.4chan.org/g/thread/42 target~?rep?~needle {}[/code]",
                "prefix ".repeat(300),
                "code prefix ".repeat(200),
                "tail ".repeat(300)
            ),
            Profile::Global,
            None,
        );
        for query in [">>42", "targetneedle"] {
            let excerpt = lines(&original, query, 64);
            let value = projection(&excerpt);
            assert!(
                value.contains(&board_domain::source_html_entities(query)),
                "{value}"
            );
            assert!(value.starts_with("<pre class=\"prettyprint\">"));
            assert!(value.ends_with("</pre>"));
            assert!(value.len() < 400);
        }
    }

    #[test]
    fn frozen_filter_choices_entities_and_quote_eligibility_survive_clipping() {
        let original = prepared(
            &format!(
                "{}\n~?rep?~> {} [code]owned needle & \"[/code] {}",
                "x".repeat(35),
                "prefix ".repeat(200),
                "tail ".repeat(200)
            ),
            Profile::Test,
            Some(LeetRolls::from_choices(0, 3).unwrap()),
        );
        let excerpt = lines(&original, "0wned needle", 64);
        let value = projection(&excerpt);
        assert!(
            value.contains("<pre cl4ss=\"prettyprint\">0wned needle &amp;4mp; &amp;qu0t;</pre>"),
            "{value}"
        );
        assert!(!value.contains("class=\"quote\""));
        assert_eq!(lines(&original, "0wned", 0).len(), 0);
    }
}
