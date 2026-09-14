use board_domain::parse_post_comment;

// Preserve source tab expansion while bounding independent synthetic inputs.
const SUBJECT_SCALARS: usize = board_domain::MAX_SUBJECT_BYTES;

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

pub(crate) fn compose(subject: &str, teaser: &str) -> String {
    if subject.is_empty() {
        return teaser.to_owned();
    }
    let subject: String = subject.chars().take(SUBJECT_SCALARS).collect();
    let mut output = String::from("<b>");
    escape_into(&mut output, &subject);
    output.push_str("</b>");
    if !teaser.is_empty() {
        output.push_str(": ");
        output.push_str(teaser);
    }
    output
}

pub(crate) fn from_post(
    subject: &str,
    comment: &str,
    format: i16,
    board: &board_store::Board,
) -> String {
    compose(
        subject,
        &super::teaser::prepare(
            &parse_post_comment(comment, format),
            &board.slug,
            board.into(),
        )
        .serialized,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::filter::Filter;
    use board_domain::parse_comment;
    use proptest::prelude::*;
    use serde::Deserialize;

    fn from_parts(subject: &str, lines: &[board_domain::Line]) -> String {
        compose(
            subject,
            &super::super::teaser::prepare(lines, "demo", Default::default()).serialized,
        )
    }

    fn from_post(subject: &str, comment: &str, format: i16) -> String {
        from_parts(subject, &parse_post_comment(comment, format))
    }

    fn from_raw(subject: &str, comment: &str) -> String {
        from_post(subject, comment, 0)
    }

    #[test]
    fn stamped_markup_has_one_server_and_browser_search_representation() {
        for format in 8..=15 {
            let comment = "[spoiler]first\n<b>second</b>[/spoiler]";
            let text = from_post("subject", comment, format);
            assert_eq!(
                text,
                from_parts("subject", &parse_post_comment(comment, format))
            );
            assert_eq!(
                text,
                if format & 1 != 0 {
                    "<b>subject</b>: <s>first &lt;b&gt;second&lt;/b&gt;</s>"
                } else {
                    "<b>subject</b>: [spoiler]first &lt;b&gt;second&lt;/b&gt;[/spoiler]"
                }
            );
        }
        assert_eq!(from_post("", "[spoiler] \n[/spoiler]", 9), "");
        assert_eq!(
            from_post("", "[code]first\nsecond[/code]", 10),
            "first second"
        );
        assert_eq!(from_post("", "[sjis]a  b\nc[/sjis]", 12), "a  b c");
    }

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
    fn expanded_subject_is_not_cut_at_the_old_storage_bound() {
        let expanded = format!("A{}B", " ".repeat(392));
        assert_eq!(
            from_raw(&expanded, "body"),
            format!("<b>{expanded}</b>: body")
        );
        assert!(Filter::new("B</b>").matches(&from_raw(&expanded, "body")));
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
