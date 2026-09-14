use board_domain::{CommentSpacing, prepare_post_content};

const MISSING: &str = "Error: New threads require a subject.";

#[test]
fn required_subject_checks_cleaned_bytes_without_a_second_trim() {
    for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
        let spacing = CommentSpacing::for_board("qst", code, sjis);
        for raw in [
            "",
            " |　| ",
            "##",
            "😀",
            "\u{200b}",
            "\r\n",
            "\t",
            "\u{31350}",
        ] {
            for attached in [false, true] {
                assert_eq!(
                    prepare_post_content("", raw, "content", 1000, attached, spacing, true)
                        .err()
                        .unwrap()
                        .0,
                    MISSING,
                    "{raw:?}"
                );
            }
            // Caller disables the rule for replies and optional-subject boards.
            assert!(prepare_post_content("", raw, "content", 1000, false, spacing, false).is_ok());
        }
        for (raw, expected) in [
            ("Ｚ##ⓦ", "aw"),
            ("#", "#"),
            ("\u{31350} \u{31350}", " "),
            ("\t|\t", "|"),
        ] {
            let prepared =
                prepare_post_content("", raw, "content", 1000, false, spacing, true).unwrap();
            assert_eq!(prepared.subject, expected);
            assert_eq!(prepared.comment, "content");
        }
        assert_eq!(
            prepare_post_content("", "│", "content", 1000, false, spacing, true).is_ok(),
            sjis
        );
    }
}

#[test]
fn raw_limits_precede_required_subject_which_precedes_comment_admission() {
    let spacing = CommentSpacing::for_board("vg", false, false).with_line_rules(1, true);
    for (name, subject, comment, limit, error) in [
        (
            "n".repeat(101),
            "".into(),
            "content".into(),
            1000,
            "Name or subject is too long.",
        ),
        (
            "".into(),
            "#".repeat(101),
            "content".into(),
            1000,
            "Name or subject is too long.",
        ),
        (
            "".into(),
            "".into(),
            "xx".into(),
            1,
            "Enter a comment within this board's character limit.",
        ),
        (
            "".into(),
            "".into(),
            "\u{1}".into(),
            1000,
            "Unsupported control character.",
        ),
        ("".into(), "".into(), "".into(), 1000, MISSING),
        ("".into(), "##".into(), "a\nb\nc".into(), 1000, MISSING),
        (
            "".into(),
            "😀".into(),
            format!("{}end", "x\n".repeat(7)),
            1000,
            MISSING,
        ),
        (
            "".into(),
            "valid".into(),
            "a\nb\nc".into(),
            1000,
            "Error: Too many lines.",
        ),
    ] {
        assert_eq!(
            prepare_post_content(&name, &subject, &comment, limit, false, spacing, true)
                .err()
                .unwrap()
                .0,
            error
        );
    }
    assert!(prepare_post_content("", "valid", "", 1000, false, spacing, true).is_err());
    assert!(prepare_post_content("", "valid", "", 1000, true, spacing, true).is_ok());
}
