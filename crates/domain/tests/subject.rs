use board_domain::{CommentSpacing, MAX_SUBJECT_BYTES, prepare_post_subject, source_html_entities};
use proptest::prelude::*;

#[test]
fn subjects_normalize_on_comment_exempt_boards_and_keep_source_stage_order() {
    for board in ["g", "a", "b", "jp", "vip"] {
        for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
            let policy = CommentSpacing::for_board(board, code, sjis);
            for (raw, expected) in [
                ("Ｚⓦ✘😀\u{200b}", "awx"),
                ("# ## ＃＃ x⌘y", "#   xy"),
                ("#\n#", "##"),
                ("|||", ""),
                ("|\t|", if code || sjis { "|    |" } else { "| |" }),
                ("A \r\n B", "A  B"),
                ("\u{31350} A \u{31350}", " A "),
                (">>>/g/1", ">>>/g/1"),
                ("a[spoiler]b[/spoiler]c", "a[spoiler]b[/spoiler]c"),
                ("\u{202e}text", "text"),
            ] {
                // Plain spacing collapses the spaces exposed by capcode removal.
                let expected = if raw == "# ## ＃＃ x⌘y" && !code && !sjis {
                    "# xy"
                } else {
                    expected
                };
                assert_eq!(
                    prepare_post_subject(raw, policy).unwrap(),
                    expected,
                    "{board} {code} {sjis} {raw:?}"
                );
            }
            assert_eq!(
                prepare_post_subject("│", policy).unwrap(),
                if sjis { "│" } else { "" }
            );
            assert_eq!(
                prepare_post_subject("　X　", policy).unwrap(),
                if sjis || matches!(board, "a" | "b" | "jp") {
                    "　X　"
                } else {
                    "X"
                }
            );
        }
    }
}

#[test]
fn subjects_enforce_raw_bytes_before_shortening_and_bound_expansion() {
    let policy = CommentSpacing::for_board("test", true, false);
    let raw = format!("A{}B", "\t".repeat(98));
    assert_eq!(raw.len(), 100);
    let expected = format!("A{}B", " ".repeat(392));
    assert_eq!(prepare_post_subject(&raw, policy).unwrap(), expected);
    assert!(expected.len() <= MAX_SUBJECT_BYTES);
    for raw in [format!("{raw}x"), "😀".repeat(26), "#".repeat(101)] {
        assert_eq!(
            prepare_post_subject(&raw, policy).unwrap_err().0,
            "Name or subject is too long."
        );
    }
    assert_eq!(prepare_post_subject(&"😀".repeat(25), policy).unwrap(), "");
    for raw in ["A\0B", "A\u{000c}B", "A\u{000b}B"] {
        assert_eq!(
            prepare_post_subject(raw, policy).unwrap_err().0,
            "Unsupported control character."
        );
    }
}

#[test]
fn source_json_entities_encode_text_once_without_decoding_literal_entities() {
    assert_eq!(source_html_entities("&<>\"'"), "&amp;&lt;&gt;&quot;&#039;");
    assert_eq!(
        source_html_entities("&lt;script&gt;"),
        "&amp;lt;script&amp;gt;"
    );
    assert_eq!(source_html_entities("𠮷é"), "𠮷é");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_subjects_have_bounded_single_line_text_or_explicit_input_errors(
        raw in prop::collection::vec(any::<char>(), 0..110).prop_map(|v| v.into_iter().collect::<String>()),
        code in any::<bool>(), sjis in any::<bool>()
    ) {
        match prepare_post_subject(&raw, CommentSpacing::for_board("jp", code, sjis)) {
            Ok(subject) => {
                prop_assert!(raw.len() <= 100);
                prop_assert!(subject.len() <= raw.len() * 4);
                prop_assert!(!subject.contains(['\r', '\n', '\t']));
                prop_assert!(subject.chars().all(|ch| ch as u32 <= 0x3134f));
            }
            Err(error) => prop_assert!(matches!(error.0, "Name or subject is too long." | "Unsupported control character.")),
        }
    }
}
