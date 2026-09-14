use board_domain::{CommentSpacing, ValidationError, prepare_post_comment};

fn prepare(
    raw: &str,
    max_lines: usize,
    code: bool,
    sjis: bool,
    spoilers: bool,
) -> Result<String, ValidationError> {
    prepare_post_comment(
        "",
        "",
        raw,
        16000,
        true,
        CommentSpacing::for_board("g", code, sjis).with_line_rules(max_lines, spoilers),
    )
}

#[test]
fn intra_word_spoilers_follow_byte_whitespace_case_and_nonoverlapping_matches() {
    for (raw, expected) in [
        ("a[spoiler]b[/spoiler]c", "abc"),
        ("a[spoiler][/spoiler]c", "ac"),
        (
            "a[spoiler]b[/spoiler]c[spoiler]d[/spoiler]e",
            "abc[spoiler]d[/spoiler]e",
        ),
        ("a[spoiler]b[/spoiler] c[/spoiler]d", "ab[/spoiler] cd"),
        ("a[spoiler]b\nc[/spoiler]d", "a[spoiler]b\nc[/spoiler]d"),
        ("a [spoiler]b[/spoiler] c", "a [spoiler]b[/spoiler] c"),
        ("[spoiler]b[/spoiler]c", "[spoiler]b[/spoiler]c"),
        ("a[SPOILER]b[/SPOILER]c", "a[SPOILER]b[/SPOILER]c"),
        ("　[spoiler]b[/spoiler]　", "　b　"),
        ("é[spoiler]b[/spoiler]é[spoiler]c[/spoiler]é", "ébécé"),
    ] {
        // SJIS preserves the wide-space bytes needed by the \S boundary case.
        assert_eq!(
            prepare(raw, 70, false, true, true).unwrap(),
            expected,
            "{raw:?}"
        );
        assert_eq!(
            prepare(raw, 70, false, true, false).unwrap(),
            raw,
            "disabled {raw:?}"
        );
    }
    assert_eq!(
        prepare(">>[spoiler]>[/spoiler]/g/42", 70, true, false, true).unwrap(),
        ">>42"
    );
    let raw = "a[spoiler]b[/spoiler]c";
    assert!(
        prepare_post_comment(
            "",
            "",
            raw,
            3,
            false,
            CommentSpacing::for_board("g", false, false).with_line_rules(70, true)
        )
        .is_err()
    );
}

#[test]
fn repeated_lines_use_the_gate_suffix_and_newline_run_boundaries() {
    for (raw, suffix) in [
        ("&", "amp;"),
        ("<", "lt;"),
        (">", "gt;"),
        ("\"", "quot;"),
        ("'", "#039;"),
    ] {
        let text = format!("prefix{raw}\n{}tail\nend", format!("{suffix}\n").repeat(5));
        assert_eq!(
            prepare(&text, 70, false, false, false).unwrap_err().0,
            "Error: Our system thinks your post is spam."
        );
    }
    for (text, rejected) in [
        ("x\n".repeat(6) + "end", false),
        ("x\n".repeat(6) + "tail\nend", true),
        (
            "prefixx\n".to_owned() + &"x\n".repeat(5) + "tail\nend",
            true,
        ),
        ("x\n\n".repeat(6) + "end", true),
        ("x\n\n".repeat(5) + "x\nend", false),
        ("x\n".repeat(5) + "x\n\nend", true),
        ("x\ny\n".repeat(6) + "end", false),
        ("a[spoiler][/spoiler]x\n".repeat(7) + "end", true),
    ] {
        for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
            let result = prepare(&text, 100, code, sjis, true);
            if rejected {
                assert_eq!(
                    result.unwrap_err().0,
                    "Error: Our system thinks your post is spam.",
                    "{text:?}"
                );
            } else {
                assert!(result.is_ok(), "{text:?}");
            }
        }
    }
    // Different newline runs become equal only after ordinary blank collapse.
    // The source does not run the repetition detector a second time.
    let text = (0..7)
        .map(|index| format!("x{}", "\n".repeat(4 + index % 2)))
        .collect::<String>()
        + "end";
    assert_eq!(
        prepare(&text, 70, false, false, false).unwrap(),
        "x\n".repeat(7) + "end"
    );
}

#[test]
fn total_lines_count_separators_after_cleanup_even_for_code_and_sjis() {
    for limit in [0, 3, 50, 70, 100] {
        for count in [limit, limit + 1] {
            let text = (0..=count)
                .map(|index| format!("line{index}"))
                .collect::<Vec<_>>()
                .join("\r\n");
            for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
                let result = prepare(&text, limit, code, sjis, false);
                if count == limit {
                    assert_eq!(result.unwrap(), text.replace("\r\n", "\n"));
                } else {
                    assert_eq!(result.unwrap_err().0, "Error: Too many lines.");
                }
            }
        }
    }
    let raw = "A".to_owned() + &"\n".repeat(101) + "B";
    assert_eq!(prepare(&raw, 1, false, false, false).unwrap(), "A\nB");
    assert_eq!(
        prepare(&raw, 1, true, false, false).unwrap_err().0,
        "Error: Too many lines."
    );
    assert_eq!(
        prepare(&raw, 1, false, true, false).unwrap_err().0,
        "Error: Too many lines."
    );
    assert_eq!(prepare("\n\nA\n\n", 0, false, false, false).unwrap(), "A");
    assert_eq!(
        prepare("A\n😀\n\n\nB", 1, false, false, false).unwrap(),
        "A\nB"
    );
}

#[test]
fn tall_updater_fixtures_keep_fifteen_distinct_admitted_lines() {
    for label in ["Hidden-tab append", "Visible-tab append"] {
        let repeated = format!("{label}\n").repeat(15);
        assert_eq!(
            prepare(&repeated, 70, false, false, false).unwrap_err().0,
            "Error: Our system thinks your post is spam."
        );
        let distinct = (0..15)
            .map(|index| format!("{label} {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let stored = prepare(&distinct, 70, false, false, false).unwrap();
        assert_eq!(stored, distinct);
        assert_eq!(stored.lines().count(), 15);
    }
}
