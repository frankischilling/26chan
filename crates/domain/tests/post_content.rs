use board_domain::{CommentSpacing, PostKind, prepare_post_content};

const OP: PostKind = PostKind::Thread {
    subject_required: false,
};
const MISSING_OP: &str = "Error: New threads require a subject or comment.";
const MISSING_REPLY: &str = "Error: No text entered.";

#[test]
fn final_content_admission_distinguishes_op_subjects_replies_and_attachments() {
    let spacing = CommentSpacing::for_board("test", false, false).with_line_rules(100, true);
    for raw in [
        "",
        " \t\r\n",
        "😀",
        "\u{31350}",
        "[spoiler][/spoiler]",
        "[spoiler] \n[spoiler]\t[/spoiler][/spoiler]",
    ] {
        for attached in [false, true] {
            assert_eq!(
                prepare_post_content("", "", raw, 1000, attached, spacing, OP)
                    .err()
                    .unwrap()
                    .0,
                MISSING_OP,
                "{raw:?}"
            );
            let op = prepare_post_content("", "subject", raw, 1000, attached, spacing, OP).unwrap();
            assert_eq!(op.subject, "subject");
            let reply =
                prepare_post_content("", "subject", raw, 1000, attached, spacing, PostKind::Reply);
            if attached {
                assert!(reply.is_ok(), "{raw:?}");
            } else {
                assert_eq!(reply.err().unwrap().0, MISSING_REPLY, "{raw:?}");
            }
        }
    }
    // Source checks bytes, without trimming again after private-point removal.
    assert_eq!(
        prepare_post_content("", "\u{31350} \u{31350}", "", 1000, false, spacing, OP)
            .unwrap()
            .subject,
        " "
    );
}

#[test]
fn final_blank_check_uses_generated_markup_and_ascii_whitespace_only() {
    for mask in 0..8 {
        let spacing = CommentSpacing::for_board("test", mask & 2 != 0, mask & 4 != 0)
            .with_line_rules(100, mask & 1 != 0);
        for (raw, blank) in [
            ("[spoiler] \n[/spoiler]", mask & 1 != 0),
            ("[code]      [/code]", mask & 2 != 0),
            ("[code]\n[/code]", mask & 2 != 0),
            ("[code]       [/code]", false),
            ("[code]\n\n[/code]", false),
            ("[sjis][/sjis]", false),
            ("<br>", false),
            ("<s></s>", false),
            // The earlier Unicode sanitation removes NBSP even with SJIS on.
            ("\u{a0}", true),
        ] {
            let op = prepare_post_content("", "", raw, 1000, false, spacing, OP);
            assert_eq!(op.is_err(), blank, "mask={mask} raw={raw:?}");
            let reply = prepare_post_content("", "", raw, 1000, false, spacing, PostKind::Reply);
            assert_eq!(reply.is_err(), blank, "mask={mask} raw={raw:?}");
            if blank {
                assert_eq!(op.err().unwrap().0, MISSING_OP);
                assert_eq!(reply.err().unwrap().0, MISSING_REPLY);
            }
        }
    }
    let wide = CommentSpacing::for_board("a", false, false);
    // Private-point removal occurs after the source's whitespace-only cleanup.
    assert!(prepare_post_content("", "", "\u{31350}　\u{31350}", 1000, false, wide, OP).is_ok());
}

#[test]
fn raw_required_subject_spam_and_line_failures_keep_their_source_precedence() {
    let spacing = CommentSpacing::for_board("test", true, false).with_line_rules(1, true);
    for kind in [OP, PostKind::Reply] {
        for (subject, raw, limit, expected) in [
            (
                "s".repeat(101),
                "".into(),
                1000,
                "Name or subject is too long.",
            ),
            (
                "".into(),
                "\0".into(),
                1000,
                "Unsupported control character.",
            ),
            (
                "".into(),
                "[spoiler][/spoiler]".into(),
                1,
                "Enter a comment within this board's character limit.",
            ),
            (
                "valid".into(),
                "x\n".repeat(7) + "end",
                1000,
                "Error: Our system thinks your post is spam.",
            ),
            (
                "valid".into(),
                "[spoiler]\n\n[/spoiler]".into(),
                1000,
                "Error: Too many lines.",
            ),
        ] {
            assert_eq!(
                prepare_post_content("", &subject, &raw, limit, true, spacing, kind)
                    .err()
                    .unwrap()
                    .0,
                expected
            );
        }
    }
    assert_eq!(
        prepare_post_content(
            "",
            "##",
            "[spoiler]\n\n[/spoiler]",
            1000,
            true,
            spacing,
            PostKind::Thread {
                subject_required: true
            }
        )
        .err()
        .unwrap()
        .0,
        "Error: New threads require a subject."
    );
    let preserved = CommentSpacing::for_board("test", true, false).with_line_rules(16000, false);
    assert_eq!(
        prepare_post_content(
            "",
            "subject",
            &("x".to_owned() + &"\t".repeat(5000) + "y"),
            16000,
            true,
            preserved,
            OP
        )
        .err()
        .unwrap()
        .0,
        "Enter a comment within this board's character limit."
    );
}
