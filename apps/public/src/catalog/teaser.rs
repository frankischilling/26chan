//! Catalog projection of already formatted comments. Serialization is search
//! data only; the page renders the remaining typed tokens through Askama.
use board_domain::word_break::WordPart;
use board_domain::{Line, Token, comment_markup::Tag, source_html_entities};

#[derive(Clone, Copy, Default)]
pub struct Policy {
    pub text_only: bool,
    pub sjis: bool,
    pub truncate: bool,
    pub source_links: bool,
}

impl From<&board_store::Board> for Policy {
    fn from(board: &board_store::Board) -> Self {
        Self {
            text_only: board.text_only,
            sjis: board.comment_sjis_spacing,
            truncate: board.slug == "b",
            source_links: false,
        }
    }
}

impl Policy {
    pub fn for_post(board: &board_store::Board, format: i16) -> Self {
        Self {
            source_links: matches!(format, 104..=111 | 120..=127),
            ..board.into()
        }
    }
}

pub struct Prepared {
    pub lines: Vec<Line>,
    pub serialized: String,
}

/// Reconstruct the source's saved comment representation for text projections.
/// The return value is data and never grants permission to bypass HTML escaping.
pub(crate) fn stored_comment(lines: &[Line], board: &str, format: i16) -> String {
    let mut result = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            result.push_str("<br>");
        }
        if line.green {
            result.push_str("<span class=\"quote\">");
        }
        result.push_str(&serialize(
            &line.tokens,
            board,
            matches!(format, 104..=111 | 120..=127),
        ));
        if line.green {
            result.push_str("</span>");
        }
    }
    result
}

pub fn prepare(lines: &[Line], board: &str, policy: Policy) -> Prepared {
    prepare_with_randomizers(lines, board, policy, None, None)
}

pub fn prepare_with_randomizers(
    lines: &[Line],
    board: &str,
    policy: Policy,
    dice: Option<&str>,
    fortune: Option<(&str, &str)>,
) -> Prepared {
    let mut tokens = Vec::new();
    let mut previous_break = false;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 && !previous_break {
            tokens.push(Token::Text(
                if policy.text_only { "\n" } else { " " }.into(),
            ));
            previous_break = true;
        }
        if line.green {
            tokens.push(Token::OpenQuote);
        }
        tokens.extend(line.tokens.iter().cloned().map(|token| {
            if policy.source_links {
                match token {
                    Token::Quote(id) => Token::Text(format!(">>{id}")),
                    Token::CrossQuote(board, id) => Token::Text(format!(">>>/{board}/{id}")),
                    Token::PostQuote(quote) => Token::Text(quote.label().into()),
                    token => token,
                }
            } else {
                token
            }
        }));
        if line.green {
            tokens.push(Token::CloseQuote);
        }
        if line.green || !line.tokens.is_empty() {
            previous_break = false;
        }
    }
    let separator = if policy.text_only { "\n" } else { " " };
    if let Some(dice) = dice {
        let mut prefix = vec![
            Token::GeneratedBold(true),
            Token::Text(format!("{dice}{separator}")),
            Token::GeneratedBold(false),
        ];
        prefix.append(&mut tokens);
        tokens = prefix;
    }
    if let Some((text, color)) = fortune
        && board_domain::posting_randomizers::fortune_class(color).is_some()
    {
        tokens.extend([
            Token::GeneratedFortune(true, color.into()),
            Token::Text(separator.into()),
            Token::GeneratedBold(true),
            Token::Text(format!("Your fortune: {text}")),
            Token::GeneratedBold(false),
            Token::GeneratedFortune(false, color.into()),
        ]);
    }
    if policy.sjis {
        tokens = replace_sjis(tokens);
    }
    let truncated = policy.truncate
        && serialize(&tokens, board, policy.source_links)
            .chars()
            .count()
            > 300;
    if !policy.truncate || truncated {
        tokens = strip(tokens);
    }
    if truncated {
        tokens = truncate(tokens);
    }
    let serialized = serialize(&tokens, board, policy.source_links);
    Prepared {
        lines: vec![Line {
            green: false,
            tokens,
        }],
        serialized,
    }
}

fn closes_span(token: &Token) -> bool {
    matches!(
        token,
        Token::CloseQuote
            | Token::CloseMarkup(
                Tag::Sjis | Tag::Bold | Tag::Italic | Tag::Red | Tag::Green | Tag::Blue
            )
    ) || matches!(token, Token::FilteredDelimiter(delimiter) if delimiter.closes_span())
}

pub(crate) fn replace_sjis(tokens: Vec<Token>) -> Vec<Token> {
    // The source regex ends at the first closing span, even across other tag
    // kinds, and its dot does not cross a literal LF. Precompute those stops.
    let mut endings = vec![None; tokens.len()];
    let mut next = None;
    for (index, token) in tokens.iter().enumerate().rev() {
        endings[index] = next;
        if closes_span(token) {
            next = Some(index);
        }
        if matches!(token, Token::Text(text) if text.contains('\n')) {
            next = None;
        }
    }
    let mut result = Vec::new();
    let mut skip = 0;
    for (index, token) in tokens.into_iter().enumerate() {
        if index < skip {
            continue;
        }
        if (matches!(token, Token::OpenMarkup(Tag::Sjis))
            || matches!(&token, Token::FilteredDelimiter(delimiter) if delimiter.is_sjis()))
            && let Some(end) = endings[index]
        {
            result.push(Token::Text("[SJIS]".into()));
            skip = end + 1;
        } else {
            result.push(token);
        }
    }
    result
}

pub(crate) fn strip(tokens: Vec<Token>) -> Vec<Token> {
    let mut result = Vec::new();
    for token in tokens {
        match token {
            Token::Text(_) | Token::OpenMarkup(Tag::Spoiler) | Token::CloseMarkup(Tag::Spoiler) => {
                result.push(token)
            }
            Token::ChangedEntity(_) => result.push(token),
            Token::FilteredDelimiter(delimiter) if delimiter.element_name() == "s" => {
                result.push(if delimiter.opening() {
                    Token::OpenMarkup(Tag::Spoiler)
                } else {
                    Token::CloseMarkup(Tag::Spoiler)
                });
            }
            Token::FilteredDelimiter(_) => {}
            Token::Spoiler(text) | Token::Link(text) => result.push(Token::Text(text)),
            Token::WrappedLink(_, parts) | Token::ServerLink(_, parts) => {
                for part in parts {
                    if let WordPart::Text(text) = part {
                        result.push(Token::Text(text));
                    }
                }
            }
            Token::Quote(id) => result.push(Token::Text(format!(">>{id}"))),
            Token::CrossQuote(board, id) => result.push(Token::Text(format!(">>>/{board}/{id}"))),
            Token::PostQuote(quote) => result.push(Token::Text(quote.label().into())),
            Token::StaticQuote(quote, _) => result.push(Token::Text(quote.label())),
            Token::WordBreak
            | Token::OpenMarkup(_)
            | Token::CloseMarkup(_)
            | Token::OpenQuote
            | Token::CloseQuote => {}
            Token::GeneratedBold(_) | Token::GeneratedFortune(_, _) => {}
        }
    }
    result
}

fn truncate(tokens: Vec<Token>) -> Vec<Token> {
    let mut result = Vec::new();
    let mut remaining = 300;
    let mut spoilers: i32 = 0;
    for token in tokens {
        match token {
            Token::Text(text) => {
                let mut prefix = String::new();
                let mut cut = false;
                for ch in text.chars() {
                    let width = match ch {
                        '&' => 5,
                        '<' | '>' => 4,
                        '"' => 6,
                        '\'' => 6,
                        _ => 1,
                    };
                    if width > remaining {
                        cut = true;
                        break;
                    }
                    prefix.push(ch);
                    remaining -= width;
                }
                if !prefix.is_empty() {
                    result.push(Token::Text(prefix));
                }
                if cut {
                    break;
                }
            }
            Token::OpenMarkup(Tag::Spoiler) | Token::CloseMarkup(Tag::Spoiler) => {
                let open = matches!(token, Token::OpenMarkup(_));
                let width = if open { 3 } else { 4 };
                if width > remaining {
                    break;
                }
                remaining -= width;
                spoilers += if open { 1 } else { -1 };
                result.push(token);
            }
            Token::ChangedEntity(entity) => {
                let width = entity.spelling().chars().count();
                if width > remaining {
                    // Source removes every incomplete trailing entity,
                    // including the finite spellings changed by /test/.
                    break;
                }
                remaining -= width;
                result.push(Token::ChangedEntity(entity));
            }
            _ => unreachable!("truncation follows tag stripping"),
        }
    }
    for _ in 0..spoilers {
        result.push(Token::CloseMarkup(Tag::Spoiler));
    }
    // Source retains its pre-strip length test, including when stripping made
    // the whole remaining text shorter than the truncation limit.
    result.push(Token::Text("…".into()));
    result
}

fn markup(tag: Tag, open: bool) -> &'static str {
    match (tag, open) {
        (Tag::Spoiler, true) => "<s>",
        (Tag::Spoiler, false) => "</s>",
        (Tag::Code, true) => "<pre class=\"prettyprint\">",
        (Tag::Code, false) => "</pre>",
        (Tag::Sjis, true) => "<span class=\"sjis\">",
        (Tag::Bold, true) => "<span class=\"mu-s\">",
        (Tag::Italic, true) => "<span class=\"mu-i\">",
        (Tag::Red, true) => "<span class=\"mu-r\">",
        (Tag::Green, true) => "<span class=\"mu-g\">",
        (Tag::Blue, true) => "<span class=\"mu-b\">",
        (_, false) => "</span>",
    }
}

pub(crate) fn serialize(tokens: &[Token], board: &str, source_links: bool) -> String {
    let mut result = String::new();
    for token in tokens {
        match token {
            Token::Text(text) => result.push_str(&source_html_entities(text)),
            Token::Quote(id) => result.push_str(&format!("<a class=\"quotelink\" href=\"/{}/post/{id}\">&gt;&gt;{id}</a>", source_html_entities(board))),
            Token::CrossQuote(target, id) => result.push_str(&format!("<a class=\"quotelink\" href=\"/{}/post/{id}\">&gt;&gt;&gt;/{}/{id}</a>", source_html_entities(target), source_html_entities(target))),
            Token::PostQuote(quote) => result.push_str(&source_html_entities(quote.label())),
            Token::StaticQuote(quote, parts) => {
                if source_links {
                    result.push_str(&format!("<a href=\"{}\" class=\"quotelink\"{}>", source_html_entities(&quote.source_href()), if quote.opens_new_tab() { " target=\"_blank\"" } else { "" }));
                } else {
                    result.push_str(&format!("<a class=\"quotelink\" href=\"{}\"{}>", source_html_entities(&quote.href()), if quote.opens_new_tab() { " target=\"_blank\" rel=\"noopener noreferrer\"" } else { "" }));
                }
                for part in parts { match part { WordPart::Text(text) => result.push_str(&source_html_entities(text)), WordPart::Break => result.push_str("<wbr>") } }
                result.push_str("</a>");
            }
            Token::Spoiler(text) => result.push_str(&format!("<span class=\"spoiler\" tabindex=\"0\" aria-label=\"Spoiler; focus to reveal\">{}</span>", source_html_entities(text))),
            Token::Link(url) => result.push_str(&format!("<a href=\"{}\" rel=\"nofollow noreferrer noopener\">{}</a>", source_html_entities(url), source_html_entities(url))),
            Token::WrappedLink(url, parts) => {
                result.push_str(&format!("<a href=\"{}\" rel=\"nofollow noreferrer noopener\">", source_html_entities(url)));
                for part in parts { match part { WordPart::Text(text) => result.push_str(&source_html_entities(text)), WordPart::Break => result.push_str("<wbr>") } }
                result.push_str("</a>");
            }
            Token::ServerLink(link, parts) => {
                result.push_str(&format!("<a href=\"{}\" target=\"_blank\"{}>", source_html_entities(link.href()), if source_links { "" } else { " rel=\"nofollow noreferrer noopener\"" }));
                for part in parts { match part { WordPart::Text(text) => result.push_str(&source_html_entities(text)), WordPart::Break => result.push_str("<wbr>") } }
                result.push_str("</a>");
            }
            Token::WordBreak => result.push_str("<wbr>"),
            Token::OpenMarkup(tag) => result.push_str(markup(*tag, true)),
            Token::CloseMarkup(tag) => result.push_str(markup(*tag, false)),
            Token::FilteredDelimiter(delimiter) => result.push_str(&delimiter.source_projection()),
            Token::ChangedEntity(entity) => result.push_str(entity.spelling()),
            Token::OpenQuote => result.push_str("<span class=\"quote\">"),
            Token::CloseQuote => result.push_str("</span>"),
            Token::GeneratedBold(open) => result.push_str(if *open { "<b>" } else { "</b>" }),
            Token::GeneratedFortune(true, color) => result.push_str(&format!("<span class=\"fortune\" style=\"color:{color}\">")),
            Token::GeneratedFortune(false, _) => result.push_str("</span>"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::parse_post_comment;

    #[test]
    fn filtered_teasers_match_the_independently_extracted_source() {
        use board_domain::wordfilter::{LeetRolls, Profile};
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/wordfilter-posting-reference.json"
        ))
        .unwrap();
        let markup = board_domain::comment_markup::MarkupPolicy {
            spoilers: true,
            code: true,
            sjis: true,
            op: true,
        };
        for (name, profile) in [
            ("global", Profile::Global),
            ("ck", Profile::Basic),
            ("asp", Profile::Asp),
            ("v", Profile::Video),
            ("test", Profile::Test),
        ] {
            for case in fixture["profiles"][name].as_array().unwrap() {
                let rolls = if profile == Profile::Test {
                    Some(
                        LeetRolls::from_choices(
                            case["rolls"][0].as_u64().unwrap() as u8,
                            case["rolls"][1].as_u64().unwrap() as u8,
                        )
                        .unwrap(),
                    )
                } else {
                    None
                };
                let mut saved = board_domain::wordfiltered_comment::prepare(
                    case["admission_input"].as_str().unwrap(),
                    markup,
                    profile,
                    rolls,
                )
                .unwrap();
                saved.freeze_format("g");
                let lines = board_domain::filtered_formatting::lines(&saved, "g");
                for (truncate, field) in [(false, "teaser_full"), (true, "teaser")] {
                    assert_eq!(
                        prepare(
                            &lines,
                            "g",
                            Policy {
                                sjis: true,
                                source_links: true,
                                truncate,
                                ..Policy::default()
                            }
                        )
                        .serialized,
                        case[field].as_str().unwrap(),
                        "{name} {rolls:?} {} {field}",
                        case["input"]
                    );
                }
            }
        }
    }

    #[test]
    fn generated_randomizers_follow_source_teaser_stripping_and_truncation() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/randomizer-reference.json"
        ))
        .unwrap();
        for case in reference["teaser_cases"].as_array().unwrap() {
            let lines = vec![Line {
                green: false,
                tokens: vec![Token::Text(case["comment"].as_str().unwrap().into())],
            }];
            let prepared = prepare_with_randomizers(
                &lines,
                "b",
                Policy {
                    truncate: case["truncate"].as_bool().unwrap(),
                    ..Default::default()
                },
                case["dice"].as_str(),
                case["fortune"].as_str().zip(case["color"].as_str()),
            );
            assert_eq!(prepared.serialized, case["teaser"], "{case}");
        }
    }
    use proptest::prelude::*;

    #[test]
    fn source_catalog_serialization_matches_the_original_php_transformations() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../../fixtures/format-reference.json"))
                .unwrap();
        for case in reference["teaser_cases"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let lines = board_domain::parse_post_comment_on_board(
                input,
                case["format"].as_i64().unwrap() as i16,
                "g",
            );
            let policy = Policy {
                text_only: case["text_only"].as_bool().unwrap(),
                sjis: true,
                truncate: case["truncate"].as_bool().unwrap(),
                source_links: true,
            };
            assert_eq!(
                prepare(&lines, "g", policy).serialized,
                case["teaser"].as_str().unwrap(),
                "{case}"
            );
        }
    }

    fn prepared(input: &str, format: i16, policy: Policy) -> Prepared {
        prepare(&parse_post_comment(input, format), "b", policy)
    }

    #[test]
    fn source_teasers_keep_spoilers_and_literal_whitespace_but_remove_generated_tags() {
        let input = "[spoiler]hidden[/spoiler]\n\n> green\n[b]bold[/b]  <script>&'\"";
        assert_eq!(
            prepared(input, 57, Policy::default()).serialized,
            "<s>hidden</s> &gt; green bold  &lt;script&gt;&amp;&#039;&quot;"
        );
        let policy = Policy {
            text_only: true,
            ..Default::default()
        };
        assert_eq!(
            prepared("a\n\n\nb  \u{a0}c", 40, policy).serialized,
            "a\nb  \u{a0}c"
        );
        assert_eq!(
            prepared("[spoiler]old[/spoiler]", 0, Policy::default()).serialized,
            "old"
        );
    }

    #[test]
    fn sjis_replacement_obeys_line_conversion_and_first_span_close() {
        let policy = Policy {
            sjis: true,
            ..Default::default()
        };
        assert_eq!(
            prepared("before[sjis]a\nb[/sjis]after", 44, policy).serialized,
            "before[SJIS]after"
        );
        assert_eq!(
            prepared(
                "[sjis]a\nb[/sjis]",
                44,
                Policy {
                    text_only: true,
                    ..policy
                }
            )
            .serialized,
            "a\nb"
        );
        assert_eq!(
            prepared("[sjis]a[b]b[/b]c[/sjis]tail", 60, policy).serialized,
            "[SJIS]ctail"
        );
        assert_eq!(
            prepared(
                "[sjis]a\n[b]b[/b]c[/sjis]",
                60,
                Policy {
                    text_only: true,
                    ..policy
                }
            )
            .serialized,
            "a\nbc"
        );
        assert_eq!(
            prepared("[sjis]a[/sjis][sjis]b[/sjis]", 44, policy).serialized,
            "[SJIS][SJIS]"
        );
        assert_eq!(
            prepared("literal <span class=\"sjis\">safe</span>", 40, policy).serialized,
            "literal &lt;span class=&quot;sjis&quot;&gt;safe&lt;/span&gt;"
        );
    }

    #[test]
    fn b_truncates_serialized_scalars_and_repairs_cut_entities_and_spoilers() {
        let policy = Policy {
            truncate: true,
            ..Default::default()
        };
        for (input, expected) in [
            ("x".repeat(300), "x".repeat(300)),
            ("x".repeat(301), format!("{}…", "x".repeat(300))),
            (
                format!("{}&tail", "x".repeat(298)),
                format!("{}…", "x".repeat(298)),
            ),
            (
                format!("{}&tail", "x".repeat(295)),
                format!("{}&amp;…", "x".repeat(295)),
            ),
            (
                format!("{}[spoiler]end[/spoiler]", "x".repeat(298)),
                format!("{}…", "x".repeat(298)),
            ),
            (
                format!("[spoiler]{}[/spoiler]", "界".repeat(301)),
                format!("<s>{}</s>…", "界".repeat(297)),
            ),
            (
                format!("[spoiler][spoiler]{}[/spoiler][/spoiler]", "x".repeat(301)),
                format!("<s><s>{}</s></s>…", "x".repeat(294)),
            ),
        ] {
            // The earlier saved formatter isolates truncation from word breaks.
            assert_eq!(prepared(&input, 9, policy).serialized, expected);
        }
        let short = prepared("[b]short[/b]", 24, policy);
        assert_eq!(short.serialized, "<span class=\"mu-s\">short</span>");
        assert_eq!(
            prepared(&"x".repeat(300), 40, policy).serialized,
            format!("{}…", "x".repeat(300))
        );
        // Stale pre-strip length deliberately adds an ellipsis even though the
        // source tag removal leaves fewer than 300 characters.
        let input = "[b]a[/b]".repeat(12);
        assert_eq!(
            prepared(&input, 24, policy).serialized,
            format!("{}…", "a".repeat(12))
        );
        let input = format!("[sjis]{}[/sjis]", "界".repeat(400));
        assert_eq!(
            prepared(
                &input,
                44,
                Policy {
                    sjis: true,
                    ..policy
                }
            )
            .serialized,
            "[SJIS]"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn literal_projection_is_bounded_escaped_and_keeps_complete_entities(input in ".{0,1000}") {
            let lines = vec![Line { green: false, tokens: vec![Token::Text(input.clone())] }];
            let full = prepare(&lines, "demo", Policy::default());
            prop_assert_eq!(&full.serialized, &source_html_entities(&input));
            prop_assert!(!full.serialized.contains('<'));
            let limited = prepare(&lines, "b", Policy { truncate: true, ..Default::default() });
            prop_assert!(limited.serialized.chars().count() <= 301);
            prop_assert!(!limited.serialized.contains('<'));
            for entity in limited.serialized.split('&').skip(1) {
                prop_assert!(entity.starts_with("amp;") || entity.starts_with("lt;") || entity.starts_with("gt;") || entity.starts_with("quot;") || entity.starts_with("#039;"));
            }
        }
    }
}
