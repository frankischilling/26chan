use board_domain::{CommentSpacing, PostKind, prepare_post_content};

const OP: PostKind = PostKind::Thread {
    staff_image_only: false,
    subject_required: false,
    text_only: false,
};
const MISSING_OP: &str = "Error: New threads require a subject or comment.";
const MISSING_REPLY: &str = "Error: No text entered.";

#[test]
fn existing_internal_markers_are_removed_once_before_final_markup_admission() {
    let spacing = CommentSpacing::for_board("g", true, true).with_line_rules(100, true);
    let content = prepare_post_content(
        "",
        "",
        "[co~?rep?~de]soy fam CUCK[/code]",
        1000,
        false,
        spacing,
        OP,
    )
    .unwrap();
    assert_eq!(content.comment, "[code]soy fam CUCK[/code]");
    let content =
        prepare_post_content("", "", "~?re~?rep?~p?~fam", 1000, false, spacing, OP).unwrap();
    assert_eq!(content.comment, "~?rep?~fam");
    for kind in [OP, PostKind::Reply] {
        let error = prepare_post_content(
            "",
            "",
            "~?rep?~[spoiler] [/spoiler]~?erep?~",
            1000,
            false,
            spacing,
            kind,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.0,
            if matches!(kind, PostKind::Reply) {
                MISSING_REPLY
            } else {
                MISSING_OP
            }
        );
    }
    assert_eq!(
        prepare_post_content("", "", "~?rep?~ok", 2, false, spacing, OP)
            .err()
            .unwrap()
            .0,
        "Enter a comment within this board's character limit."
    );
}

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
                staff_image_only: false,
                subject_required: true,
                text_only: false
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

#[test]
fn source_staff_image_only_exception_preserves_early_subject_and_attachment_rules() {
    // imgboard.php final admission uses the pre-capcode Options field, an
    // attachment and moderator authority. REQUIRE_SUBJECT is checked earlier;
    // the later TEXT_ONLY subject check is the nonblank-comment else branch.
    let spacing = CommentSpacing::for_board("test", false, false).with_line_rules(100, true);
    for staff_image_only in [false, true] {
        for has_attachment in [false, true] {
            for subject_required in [false, true] {
                for text_only in [false, true] {
                    let kind = PostKind::Thread {
                        staff_image_only,
                        subject_required,
                        text_only,
                    };
                    for comment in ["", " \n\t", "[spoiler] [/spoiler]"] {
                        let result = prepare_post_content(
                            "",
                            "",
                            comment,
                            1000,
                            has_attachment,
                            spacing,
                            kind,
                        );
                        if subject_required {
                            assert_eq!(
                                result.err().unwrap().0,
                                "Error: New threads require a subject."
                            );
                        } else if staff_image_only && has_attachment {
                            assert!(result.is_ok());
                        } else {
                            assert_eq!(result.err().unwrap().0, MISSING_OP);
                        }
                    }
                    let result = prepare_post_content(
                        "",
                        "",
                        "nonblank",
                        1000,
                        has_attachment,
                        spacing,
                        kind,
                    );
                    assert_eq!(result.is_err(), subject_required || text_only);
                }
            }
        }
    }
}

#[test]
fn pinned_source_final_admission_vectors_match() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/post-content-admission.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 192);
    let spacing = CommentSpacing::for_board("test", false, false);
    for case in cases {
        let kind = if case["resto"] == 0 {
            PostKind::Thread {
                staff_image_only: case["moderator"].as_bool().unwrap()
                    && !case["options_field"].as_str().unwrap().is_empty(),
                subject_required: false,
                text_only: case["text_only"].as_bool().unwrap(),
            }
        } else {
            PostKind::Reply
        };
        let result = prepare_post_content(
            "",
            case["sub"].as_str().unwrap(),
            case["com"].as_str().unwrap(),
            1000,
            case["has_image"].as_bool().unwrap(),
            spacing,
            kind,
        );
        let outcome = match result {
            Ok(_) => "allow",
            Err(error) => match error.0 {
                MISSING_OP => "op_text",
                MISSING_REPLY => "reply_text",
                "Error: New threads require a subject." => "subject",
                other => panic!("Unexpected admission error: {other}"),
            },
        };
        assert_eq!(outcome, case["outcome"].as_str().unwrap(), "{case}");
    }
}
