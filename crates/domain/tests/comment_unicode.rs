use board_domain::{CommentSpacing, prepare_post_comment};
use proptest::prelude::*;

fn prepare(text: &str, board: &str, code: bool, sjis: bool) -> String {
    prepare_post_comment(
        "",
        "",
        text,
        16000,
        true,
        CommentSpacing::for_board(board, code, sjis),
    )
    .unwrap()
}

#[test]
fn mapping_keeps_source_oddities_omissions_and_board_exceptions() {
    let raw = "ＺℤℨɪʟℒℳＡαØøκ𠮷ⓦ✘";
    // The active source cases map Z-like letters to a, small-cap i to J,
    // small-cap/script L to M, and script M to N. Greek alpha stays Greek.
    assert_eq!(prepare(raw, "g", true, false), "aaaJMMNAαØøκ𠮷wx");
    assert_eq!(prepare(raw, "b", false, false), "aaaJMMNAαØøκ𠮷wx");
    for (board, sjis) in [("a", false), ("jp", false), ("vip", true), ("g", true)] {
        assert_eq!(prepare(raw, board, false, sjis), "ＺℤℨɪʟℒℳＡαØøκ𠮷");
    }
    // A blanket fullwidth or compatibility normalizer would convert these.
    assert_eq!(prepare("［］＼＿｀ﬁ①", "g", true, false), "［］＼＿｀ﬁ");
    assert_eq!(prepare("＜script＞ ＆", "g", true, false), "<script> &");
}

#[test]
fn emoticons_box_drawing_and_private_ceiling_follow_source_order() {
    let once = prepare("\u{3000}\r\n", "vip", false, true);
    assert_eq!(once, "\u{3000}");
    assert_eq!(prepare(&once, "vip", false, true), "");
    let raw = "A😀☀🫠\u{2312}─━│\u{3134f}\u{31350}\u{10ffff}B";
    assert_eq!(prepare(raw, "g", true, false), "A\u{2312}─━\u{3134f}B");
    assert_eq!(prepare(raw, "a", false, false), "A\u{2312}─━\u{3134f}B");
    assert_eq!(prepare(raw, "vip", false, true), "A\u{2312}─━│\u{3134f}B");
    assert_eq!(prepare("\u{31350} A \u{31350}", "g", true, false), " A ");
    // Zero-width tags are removed before trim only on non-exempt boards.
    assert_eq!(prepare("\u{e0001} A \u{e0001}", "g", true, false), "A");
    assert_eq!(prepare("\u{e0001} A \u{e0001}", "vip", false, true), " A ");
    assert_eq!(
        prepare("A\n😀\n\u{31350}\n\nB", "demo", false, false),
        "A\nB"
    );
    for (board, sjis) in [("g", false), ("a", false), ("vip", true)] {
        assert_eq!(prepare(" |　\t| ", board, false, sjis), "");
        assert_eq!(prepare(" || X || ", board, false, sjis), "|| X ||");
    }
}

#[test]
fn removed_characters_still_count_before_cleanup_and_need_attachment_authority() {
    for spacing in [
        CommentSpacing::for_board("g", true, false),
        CommentSpacing::for_board("vip", false, true),
    ] {
        for raw in ["😀", "\u{31350}", " |　| "] {
            assert!(prepare_post_comment("", "", raw, 100, false, spacing).is_err());
            assert_eq!(
                prepare_post_comment("", "", raw, 100, true, spacing).unwrap(),
                ""
            );
        }
        assert_eq!(
            prepare_post_comment("", "", "😀X", 2, false, spacing).unwrap(),
            "X"
        );
        assert!(prepare_post_comment("", "", "😀X", 1, false, spacing).is_err());
        assert!(prepare_post_comment("", "", &"😀".repeat(16001), 16000, true, spacing).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_unicode_cleanup_is_bounded_without_assuming_idempotence(
        chars in prop::collection::vec(any::<char>(), 0..512), code in any::<bool>(), sjis in any::<bool>()
    ) {
        let raw: String = chars.into_iter().collect();
        if let Ok(text) = prepare_post_comment("", "", &raw, 16000, true, CommentSpacing::for_board("g", code, sjis)) {
            prop_assert!(text.len() <= raw.len() * 4);
            prop_assert!(text.chars().all(|ch| ch as u32 <= 0x3134f));
            prop_assert!(!text.contains('\r'));
            prop_assert!(!text.contains('😀'));
        }
    }
}
