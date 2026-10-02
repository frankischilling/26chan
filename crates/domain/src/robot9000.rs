//! Robot9000 comment reduction and user-visible mute wording.
//! Persistence and board policy stay in the store crate.

use ring::digest::{SHA256, digest};

use crate::Token;
use crate::comment_markup::MarkupPolicy;
use crate::word_break::WordPart;

pub const SIGNAL_RATIO: f64 = 0.1;
pub const SIGNAL_MIN_LEN: usize = 10;
pub const MAX_MUTE_SECONDS: i64 = 31_536_000;
pub const DEMUTE_PERIOD_SECONDS: i64 = 86_400;
pub const MAX_INPUT_BYTES: usize = 131_072;

pub const EMPTY_COMMENT: &str = "Textless posts are not allowed.";
pub const ASCII_ONLY: &str = "Non-ASCII text is not allowed.";
pub const DUPLICATE_TEXT: &str = "your comment was not original.";
pub const DUPLICATE_IMAGE: &str = "your image was not original.";
pub const INPUT_TOO_LARGE: &str = "Robot9000 comment is too large.";

#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    pub digest: [u8; 32],
    pub normalized: String,
    pub low_signal_percent: Option<f64>,
}

/// Project the prepared stored comment into the sanitized HTML shape seen by
/// the source plugin. User text is escaped; generated markup and line breaks
/// are HTML and disappear in the plugin's strip-HTML pass.
pub fn prepare_post(
    comment: &str,
    policy: MarkupPolicy,
    board: &str,
) -> Result<Prepared, &'static str> {
    if comment.len() > MAX_INPUT_BYTES {
        return Err(INPUT_TOO_LARGE);
    }
    let format = 104
        | i16::from(policy.spoilers)
        | (i16::from(policy.code) << 1)
        | (i16::from(policy.sjis) << 2)
        | (i16::from(policy.op) << 4);
    let mut source = String::with_capacity(comment.len());
    for (index, line) in crate::parse_post_comment_on_board(comment, format, board)
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            source.push_str("<br>");
        }
        if line.green {
            source.push_str("<span class=\"quote\">");
        }
        for token in line.tokens {
            match token {
                Token::Text(text) => escape_source_text(&text, &mut source),
                Token::Quote(id) => source.push_str(&format!("&gt;&gt;{id}")),
                Token::CrossQuote(board, id) => {
                    source.push_str(&format!("&gt;&gt;&gt;/{board}/{id}"))
                }
                Token::PostQuote(quote) => escape_source_text(quote.label(), &mut source),
                Token::StaticQuote(quote, parts) => {
                    source.push_str("<a class=\"quotelink\" href=\"");
                    escape_source_text(&quote.source_href(), &mut source);
                    source.push_str("\">");
                    append_parts(parts, &mut source);
                    source.push_str("</a>");
                }
                Token::Spoiler(text) => {
                    source.push_str("<s>");
                    escape_source_text(&text, &mut source);
                    source.push_str("</s>");
                }
                Token::Link(url) => {
                    source.push_str("<a href=\"");
                    escape_source_text(&url, &mut source);
                    source.push_str("\">");
                    escape_source_text(&url, &mut source);
                    source.push_str("</a>");
                }
                Token::WrappedLink(url, parts) => {
                    source.push_str("<a href=\"");
                    escape_source_text(&url, &mut source);
                    source.push_str("\">");
                    append_parts(parts, &mut source);
                    source.push_str("</a>");
                }
                Token::ServerLink(link, parts) => {
                    source.push_str("<a href=\"");
                    escape_source_text(link.href(), &mut source);
                    source.push_str("\" target=\"_blank\">");
                    append_parts(parts, &mut source);
                    source.push_str("</a>");
                }
                Token::WordBreak => source.push_str("<wbr>"),
                Token::OpenMarkup(tag) => source.push_str(markup(tag, true)),
                Token::CloseMarkup(tag) => source.push_str(markup(tag, false)),
                Token::FilteredDelimiter(delimiter) => {
                    source.push_str(&delimiter.source_projection())
                }
                Token::ChangedEntity(entity) => source.push_str(entity.spelling()),
                Token::OpenQuote => source.push_str("<span class=\"quote\">"),
                Token::CloseQuote => source.push_str("</span>"),
                Token::GeneratedBold(_) | Token::GeneratedFortune(_, _) => {
                    return Err("Invalid Robot9000 comment projection.");
                }
            }
        }
        if line.green {
            source.push_str("</span>");
        }
    }
    prepare(&source)
}

fn append_parts(parts: Vec<WordPart>, output: &mut String) {
    for part in parts {
        match part {
            WordPart::Text(text) => escape_source_text(&text, output),
            WordPart::Break => output.push_str("<wbr>"),
        }
    }
}

pub(crate) fn markup(tag: crate::comment_markup::Tag, open: bool) -> &'static str {
    use crate::comment_markup::Tag;
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

fn escape_source_text(input: &str, output: &mut String) {
    for ch in input.chars() {
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

/// Preserve the active plugin's check order: exact empty and non-ASCII checks
/// happen before mute lookup; signal evaluation and originality follow it.
pub fn prepare(comment: &str) -> Result<Prepared, &'static str> {
    if comment.len() > MAX_INPUT_BYTES {
        return Err(INPUT_TOO_LARGE);
    }
    if comment.is_empty() {
        return Err(EMPTY_COMMENT);
    }
    if !comment.is_ascii() {
        return Err(ASCII_ONLY);
    }

    let lowered = comment.to_ascii_lowercase();
    let without_html = strip_html(&lowered);
    let original_length = without_html.len();
    let without_quotes = strip_quote_links(&without_html);
    let without_entities = strip_entities(&without_quotes);
    let alnum: String = without_entities
        .bytes()
        .filter(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        .map(char::from)
        .collect();
    let without_leading_digits = alnum.trim_start_matches(|ch: char| ch.is_ascii_digit());
    let normalized = collapse_repeated(without_leading_digits);

    let low_signal_percent = if lowered.len() > SIGNAL_MIN_LEN {
        let ratio = if original_length == 0 {
            0.0
        } else {
            normalized.len() as f64 / original_length as f64
        };
        (ratio < SIGNAL_RATIO).then_some(ratio * 100.0)
    } else {
        None
    };
    let hash = digest(&SHA256, normalized.as_bytes());
    let digest = hash.as_ref().try_into().expect("SHA-256 length");
    Ok(Prepared {
        digest,
        normalized,
        low_signal_percent,
    })
}

fn strip_html(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'<' {
            if let Some(relative_end) = bytes[cursor + 1..].iter().position(|byte| *byte == b'>') {
                cursor += relative_end + 2;
                continue;
            }
            // Once no closing bracket remains, every remaining byte is text.
            // Avoid searching the same suffix for each unmatched '<'.
            output.push_str(&input[cursor..]);
            break;
        }
        output.push(char::from(bytes[cursor]));
        cursor += 1;
    }
    output
}

fn strip_quote_links(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let prefix_len = if bytes[cursor..].starts_with(b"&gt;&gt;") {
            8
        } else {
            output.push(char::from(bytes[cursor]));
            cursor += 1;
            continue;
        };
        let mut end = cursor + prefix_len;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end > cursor + prefix_len {
            cursor = end;
        } else {
            output.push(char::from(bytes[cursor]));
            cursor += 1;
        }
    }
    output
}

fn strip_entities(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'&' {
            output.push(char::from(bytes[cursor]));
            cursor += 1;
            continue;
        }
        let mut word = cursor + 1;
        if bytes.get(word) == Some(&b'#') {
            word += 1;
        }
        let start = word;
        while bytes
            .get(word)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            word += 1;
        }
        if word > start && bytes.get(word) == Some(&b';') {
            cursor = word + 1;
        } else {
            output.push('&');
            cursor += 1;
        }
    }
    output
}

fn collapse_repeated(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut bytes = input.bytes().peekable();
    while let Some(byte) = bytes.next() {
        let mut count = 1;
        while bytes.peek() == Some(&byte) {
            bytes.next();
            count += 1;
        }
        if count >= 3 {
            output.push(char::from(byte));
        } else {
            for _ in 0..count {
                output.push(char::from(byte));
            }
        }
    }
    output
}

pub fn low_signal_reason(percent: f64) -> String {
    format!("your comment was too low in content ({percent:.2}% content).")
}

pub fn violation_message(seconds: i64, reason: &str) -> String {
    format!(
        "You have been muted for {}, because {reason}",
        pretty_duration(seconds)
    )
}

pub fn pretty_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let parts = [
        (seconds / 604_800, "week"),
        ((seconds / 86_400) % 7, "day"),
        ((seconds / 3_600) % 24, "hour"),
        ((seconds / 60) % 60, "minute"),
        (seconds % 60, "second"),
    ];
    parts
        .into_iter()
        .filter(|(value, _)| *value != 0)
        .map(|(value, unit)| format!("{value} {unit}{}", if value == 1 { "" } else { "s" }))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn next_mute(timeout_power: u8) -> (u8, i64) {
    let mut power = timeout_power.min(24) + 1;
    let mut seconds = 1i64 << power;
    if seconds > MAX_MUTE_SECONDS {
        power -= 1;
        seconds = MAX_MUTE_SECONDS;
    }
    (power, seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn normalization_and_duration_match_the_extracted_source() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/robot9000-reference.json"))
                .unwrap();
        for case in reference["normalization"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let actual = prepare(input);
            if let Some(error) = case["error"].as_str() {
                assert_eq!(actual.unwrap_err(), error);
                continue;
            }
            let actual = actual.unwrap();
            assert_eq!(actual.normalized, case["normalized"], "{input:?}");
            assert_eq!(
                actual.low_signal_percent.map(low_signal_reason),
                case["reason"].as_str().map(str::to_owned),
                "{input:?}"
            );
        }
        for case in reference["durations"].as_array().unwrap() {
            assert_eq!(
                pretty_duration(case["seconds"].as_i64().unwrap()),
                case["text"]
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_text_is_bounded_and_never_produces_invalid_digests(input in ".{0,512}") {
            if let Ok(prepared) = prepare(&input) {
                prop_assert!(input.is_ascii());
                prop_assert!(prepared.normalized.len() <= input.len());
                prop_assert!(prepared.normalized.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'));
                prop_assert!(prepared.low_signal_percent.is_none_or(|ratio| ratio.is_finite() && (0.0..10.0).contains(&ratio)));
            }
        }
    }

    #[test]
    fn unmatched_html_and_large_inputs_have_explicit_bounds() {
        assert_eq!(
            prepare(&"x".repeat(MAX_INPUT_BYTES + 1)).unwrap_err(),
            INPUT_TOO_LARGE
        );
        assert_eq!(
            prepare(&"<".repeat(MAX_INPUT_BYTES)).unwrap().normalized,
            ""
        );
        assert_eq!(next_mute(u8::MAX), (24, MAX_MUTE_SECONDS));
    }

    #[test]
    fn source_normalization_order_and_quirks_are_retained() {
        assert_eq!(prepare("").unwrap_err(), EMPTY_COMMENT);
        assert_eq!(prepare("café").unwrap_err(), ASCII_ONLY);

        let prepared = prepare("123ABC456 <b>AAA</b> &gt;&gt;42 &amp; -- XXxxx!!!").unwrap();
        assert_eq!(prepared.normalized, "abc456a--x");

        assert_eq!(prepare("111abc222").unwrap().normalized, "abc2");
        assert_eq!(prepare("aabbbccccdd").unwrap().normalized, "aabcdd");
        assert_eq!(
            prepare("before <a>inside</a> after").unwrap().normalized,
            "beforeinsideafter"
        );
        assert_eq!(
            prepare("words >>123 remain").unwrap().normalized,
            "words123remain"
        );
    }

    #[test]
    fn prepared_post_escapes_user_html_and_removes_generated_markup_and_quotes() {
        let policy = MarkupPolicy {
            spoilers: true,
            code: true,
            sjis: false,
            op: true,
        };
        let prepared = prepare_post(
            "<b>User</b> >>123 [spoiler]Secret[/spoiler]\n[b]Bold[/b] &gt;",
            policy,
            "r9k",
        )
        .unwrap();
        assert_eq!(prepared.normalized, "buserbsecretboldgt");
        assert_eq!(
            prepare_post(">", MarkupPolicy::default(), "r9k")
                .unwrap()
                .low_signal_percent,
            Some(0.0)
        );
    }

    #[test]
    fn signal_ratio_uses_post_html_length_and_source_threshold() {
        let low = prepare("!!!!!!!!!!!a").unwrap();
        assert_eq!(low.normalized, "a");
        assert!((low.low_signal_percent.unwrap() - 8.333333).abs() < 0.001);

        let short = prepare("!!!!!!!!!a").unwrap();
        assert_eq!(short.low_signal_percent, None);
        let enough = prepare("abcdefghijk").unwrap();
        assert_eq!(enough.low_signal_percent, None);
    }

    #[test]
    fn progressive_mutes_and_duration_words_match_source_shape() {
        assert_eq!(next_mute(0), (1, 2));
        assert_eq!(next_mute(1), (2, 4));
        assert_eq!(next_mute(24), (24, MAX_MUTE_SECONDS));
        assert_eq!(pretty_duration(1), "1 second");
        assert_eq!(
            pretty_duration(604_800 + 86_400 + 3_600 + 60 + 1),
            "1 week 1 day 1 hour 1 minute 1 second"
        );
        assert_eq!(
            violation_message(4, DUPLICATE_TEXT),
            "You have been muted for 4 seconds, because your comment was not original."
        );
    }
}
