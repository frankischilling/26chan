use board_domain::{CommentSpacing, MAX_COMMENT_CHARS, prepare_post_comment};
use proptest::prelude::*;

fn prepare(text: &str, board: &str, code: bool, sjis: bool) -> String {
    prepare_post_comment(
        "",
        "",
        text,
        MAX_COMMENT_CHARS,
        true,
        CommentSpacing::for_board(board, code, sjis),
    )
    .unwrap()
}

#[test]
fn source_spaces_trimming_and_code_sjis_exceptions() {
    let text = " \t A\t  B\u{200b}\u{2029}C\u{a0}\u{3000}D \r\n";
    assert_eq!(prepare(text, "demo", false, false), "A BC D");
    assert_eq!(prepare(text, "g", true, false), "A      BC D");
    assert_eq!(
        prepare(text, "vip", false, true),
        "A      B\u{200b}\u{2029}C\u{3000}D"
    );
    for board in ["a", "jp"] {
        assert_eq!(prepare(text, board, false, false), "A B C\u{3000}D");
    }
    assert_eq!(prepare(text, "b", false, false), "A BC\u{3000}D");
    // PHP's ASCII trim preserves U+3000; NBSP was removed earlier.
    assert_eq!(
        prepare("\u{3000}wide\u{a0}", "b", false, false),
        "\u{3000}wide"
    );
    assert_eq!(prepare("\u{3000}wide\u{a0}", "demo", false, false), "wide");
    assert_eq!(
        prepare("<script>  & ' \"", "demo", false, false),
        "<script> & ' \""
    );
}

#[test]
fn zero_width_stage_uses_its_own_board_exceptions_and_fixed_ranges() {
    for point in [
        0x0702, 0x1d176, 0x205f, 0xfeff, 0x11a6, 0x3164, 0x2800, 0x180b, 0x180e, 0x115f, 0x1160,
        0xffa0, 0x034f, 0x17b4, 0x17b5, 0x2000, 0x200f, 0x2028, 0x202f, 0x2060, 0x206f, 0xfe00,
        0xfe0f, 0xfff0, 0xfffb, 0xe0100, 0xe01ef, 0xe0001, 0xe007f,
    ] {
        let text = format!("A{}B", char::from_u32(point).unwrap());
        assert_eq!(prepare(&text, "g", true, false), "AB", "{point:x}");
        // SJIS bypasses zero-width cleanup, but not emoticon/private cleanup.
        let removed_later = matches!(
            point,
            0x1d176 | 0x180e | 0x200f | 0x2060 | 0x206f | 0xfe00 | 0xfe0f
        ) || point > 0x3134f;
        assert_eq!(
            prepare(&text, "vip", false, true),
            if removed_later { "AB" } else { &text },
            "{point:x}"
        );
    }
    for point in [
        0x0701,
        0x0703,
        0x1d175,
        0x1d177,
        0x2000 - 1,
        0x206f + 1,
        0xfe0f + 1,
        0xffef,
        0xfffc,
        0xe00ff,
        0xe01f0,
        0xe0000,
        0xe0080,
    ] {
        let text = format!("A{}B", char::from_u32(point).unwrap());
        let removed_later = matches!(point, 0x1d175 | 0x1d177) || point > 0x3134f;
        assert_eq!(
            prepare(&text, "g", true, false),
            if removed_later {
                "AB"
            } else if point == 0x2070 {
                "A0B"
            } else {
                &text
            },
            "{point:x}"
        );
    }
    for board in ["a", "b", "jp", "g", "vip"] {
        assert_eq!(prepare("A\u{a0}\u{ad}B", board, false, true), "AB");
    }
}

#[test]
fn four_newlines_collapse_but_three_and_code_sjis_runs_remain() {
    let repeated_tabs = format!("x{}\r\nx", "\t\r\n".repeat(6));
    assert_eq!(
        prepare_post_comment(
            "",
            "",
            &repeated_tabs,
            MAX_COMMENT_CHARS,
            true,
            CommentSpacing::for_board("demo", false, false)
        )
        .unwrap_err()
        .0,
        "Error: Our system thinks your post is spam."
    );
    for count in 1..=6 {
        let input = format!("A{}B", "\n \u{3000}".repeat(count - 1) + "\n");
        let ordinary = prepare(&input, "b", false, false);
        if count >= 4 {
            assert_eq!(ordinary, "A\nB");
        } else {
            assert_eq!(ordinary, input);
        }
        assert_eq!(prepare(&input, "b", true, false), input);
        assert_eq!(prepare(&input, "vip", false, true), input);
    }
    for count in 7..=12 {
        let input = format!("A{}B", "\n \u{3000}".repeat(count - 1) + "\n");
        for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_eq!(
                prepare_post_comment(
                    "",
                    "",
                    &input,
                    MAX_COMMENT_CHARS,
                    true,
                    CommentSpacing::for_board("b", code, sjis)
                )
                .unwrap_err()
                .0,
                "Error: Our system thinks your post is spam."
            );
        }
    }
    assert_eq!(
        prepare("A\n\n\n\nB\n\n\n\nC", "demo", false, false),
        "A\nB\nC"
    );
    assert_eq!(prepare("A\n\u{a0}\n\n\nB", "b", false, false), "A\nB");
    assert_eq!(prepare("A\r\n \r \r\n\rB", "demo", false, false), "A\nB");
}

#[test]
fn raw_board_limits_and_independent_output_ceiling_still_apply() {
    let plain = CommentSpacing::for_board("demo", false, false);
    let code = CommentSpacing::for_board("g", true, false);
    assert!(prepare_post_comment("", "", " A ", 2, false, plain).is_err());
    assert_eq!(
        prepare_post_comment("", "", " A ", 3, false, plain).unwrap(),
        "A"
    );
    assert_eq!(
        prepare_post_comment("", "", "A\tB", 3, false, code).unwrap(),
        "A    B"
    );
    assert!(
        prepare_post_comment(
            "",
            "",
            &format!("A{}B", "\t".repeat(4000)),
            16000,
            false,
            code
        )
        .is_err()
    );
    assert_eq!(prepare(" \r\n\t ", "demo", false, false), "");
    assert!(prepare_post_comment("", "", " \u{200b} ", 20, false, plain).is_err());
    for text in ["A\0B", "A\u{000c}B", "A\u{000b}B"] {
        assert!(prepare_post_comment("", "", text, 20, true, plain).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn bounded_spacing_preserves_admitted_content_or_returns_a_source_line_error(
        pieces in prop::collection::vec(prop_oneof![Just("\r\n"), Just("\n"), Just("\t"), Just(" "), Just("\u{3000}"), Just("\u{200b}"), Just("x"), Just("😀")], 0..256),
        code in any::<bool>(), sjis in any::<bool>()
    ) {
        // Anchors exclude empty comments. Admission runs before blank collapse;
        // a second complete pass can therefore legitimately reject its output.
        let raw = format!("x{}x", pieces.concat());
        match prepare_post_comment("", "", &raw, MAX_COMMENT_CHARS, true,
            CommentSpacing::for_board("demo", code, sjis)) {
            Ok(cleaned) => {
                prop_assert_eq!(cleaned.matches('x').count(), raw.matches('x').count());
                prop_assert!(!cleaned.contains('😀'));
                prop_assert!(cleaned.len() <= raw.len() * 4);
                prop_assert!(!cleaned.contains('\r'));
            }
            Err(error) => prop_assert!(matches!(error.0,
                "Error: Our system thinks your post is spam." | "Error: Too many lines.")),
        }
    }
}
