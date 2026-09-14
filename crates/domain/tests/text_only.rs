use board_domain::{CommentSpacing, PostKind, prepare_post_content};

#[test]
fn text_only_requires_a_subject_after_ordinary_final_comment_admission() {
    let spacing = CommentSpacing::for_board("news", false, false).with_line_rules(2, true);
    let kind = PostKind::Thread {
        subject_required: false,
        text_only: true,
    };
    for attached in [false, true] {
        for (subject, comment, expected) in [
            ("", "", "Error: New threads require a subject or comment."),
            (
                "",
                "[spoiler][/spoiler]",
                "Error: New threads require a subject or comment.",
            ),
            ("", "valid", "Error: New threads require a subject."),
            ("##😀", "valid", "Error: New threads require a subject."),
            ("", "a\nb\nc\nd", "Error: Too many lines."),
        ] {
            assert_eq!(
                prepare_post_content("", subject, comment, 1000, attached, spacing, kind)
                    .err()
                    .unwrap()
                    .0,
                expected
            );
        }
        for raw in ["", "valid", "[spoiler][/spoiler]"] {
            let post =
                prepare_post_content("", "Owned subject", raw, 1000, attached, spacing, kind)
                    .unwrap();
            assert_eq!(post.subject, "Owned subject");
        }
    }
    // REQUIRE_SUBJECT is the earlier independent policy, even on TEXT_ONLY.
    assert_eq!(
        prepare_post_content(
            "",
            "",
            "a\nb\nc\nd",
            1000,
            false,
            spacing,
            PostKind::Thread {
                subject_required: true,
                text_only: true
            }
        )
        .err()
        .unwrap()
        .0,
        "Error: New threads require a subject."
    );
    assert!(prepare_post_content("", "", "reply", 1000, false, spacing, PostKind::Reply).is_ok());
}
