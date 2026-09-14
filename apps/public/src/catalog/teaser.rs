//! Catalog projection of already formatted comments. Serialization is search
//! data only; the page renders the remaining typed tokens through Askama.
use board_domain::word_break::WordPart;
use board_domain::{Line, Token, comment_markup::Tag, source_html_entities};

#[derive(Clone, Copy, Default)]
pub struct Policy {
    pub text_only: bool,
    pub sjis: bool,
    pub truncate: bool,
}

impl From<&board_store::Board> for Policy {
    fn from(board: &board_store::Board) -> Self {
        Self {
            text_only: board.text_only,
            sjis: board.comment_sjis_spacing,
            truncate: board.slug == "b",
        }
    }
}

pub struct Prepared {
    pub lines: Vec<Line>,
    pub serialized: String,
}

pub fn prepare(lines: &[Line], board: &str, policy: Policy) -> Prepared {
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
        tokens.extend(line.tokens.iter().cloned());
        if line.green {
            tokens.push(Token::CloseQuote);
        }
        if line.green || !line.tokens.is_empty() {
            previous_break = false;
        }
    }
    if policy.sjis {
        tokens = replace_sjis(tokens);
    }
    let truncated = policy.truncate && serialize(&tokens, board).chars().count() > 300;
    if !policy.truncate || truncated {
        tokens = strip(tokens);
    }
    if truncated {
        tokens = truncate(tokens);
    }
    let serialized = serialize(&tokens, board);
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
    )
}

fn replace_sjis(tokens: Vec<Token>) -> Vec<Token> {
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
        if matches!(token, Token::OpenMarkup(Tag::Sjis))
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

fn strip(tokens: Vec<Token>) -> Vec<Token> {
    let mut result = Vec::new();
    for token in tokens {
        match token {
            Token::Text(_) | Token::OpenMarkup(Tag::Spoiler) | Token::CloseMarkup(Tag::Spoiler) => {
                result.push(token)
            }
            Token::Spoiler(text) | Token::Link(text) => result.push(Token::Text(text)),
            Token::WrappedLink(_, parts) => {
                for part in parts {
                    if let WordPart::Text(text) = part {
                        result.push(Token::Text(text));
                    }
                }
            }
            Token::Quote(id) => result.push(Token::Text(format!(">>{id}"))),
            Token::CrossQuote(board, id) => result.push(Token::Text(format!(">>>/{board}/{id}"))),
            Token::WordBreak
            | Token::OpenMarkup(_)
            | Token::CloseMarkup(_)
            | Token::OpenQuote
            | Token::CloseQuote => {}
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

fn serialize(tokens: &[Token], board: &str) -> String {
    let mut result = String::new();
    for token in tokens {
        match token {
            Token::Text(text) => result.push_str(&source_html_entities(text)),
            Token::Quote(id) => result.push_str(&format!("<a class=\"quotelink\" href=\"/{}/post/{id}\">&gt;&gt;{id}</a>", source_html_entities(board))),
            Token::CrossQuote(target, id) => result.push_str(&format!("<a class=\"quotelink\" href=\"/{}/post/{id}\">&gt;&gt;&gt;/{}/{id}</a>", source_html_entities(target), source_html_entities(target))),
            Token::Spoiler(text) => result.push_str(&format!("<span class=\"spoiler\" tabindex=\"0\" aria-label=\"Spoiler; focus to reveal\">{}</span>", source_html_entities(text))),
            Token::Link(url) => result.push_str(&format!("<a href=\"{}\" rel=\"nofollow noreferrer noopener\">{}</a>", source_html_entities(url), source_html_entities(url))),
            Token::WrappedLink(url, parts) => {
                result.push_str(&format!("<a href=\"{}\" rel=\"nofollow noreferrer noopener\">", source_html_entities(url)));
                for part in parts { match part { WordPart::Text(text) => result.push_str(&source_html_entities(text)), WordPart::Break => result.push_str("<wbr>") } }
                result.push_str("</a>");
            }
            Token::WordBreak => result.push_str("<wbr>"),
            Token::OpenMarkup(tag) => result.push_str(markup(*tag, true)),
            Token::CloseMarkup(tag) => result.push_str(markup(*tag, false)),
            Token::OpenQuote => result.push_str("<span class=\"quote\">"),
            Token::CloseQuote => result.push_str("</span>"),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::parse_post_comment;
    use proptest::prelude::*;

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
