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
fn source_rewrite_is_literal_global_and_does_not_parse_identifiers() {
    let text = "See >>>/g/00012. >>>/g/0 >>>/g/184467440737095516160x >>>/g/1>>>/g/2";
    let expected = "See >>00012. >>0 >>184467440737095516160x >>1>>2";
    for (code, sjis) in [(false, false), (true, false), (false, true), (true, true)] {
        assert_eq!(prepare(text, "g", code, sjis), expected);
        assert_eq!(
            prepare(
                "[spoiler]>>>/g/12[/spoiler] [code]>>>/g/34[/code]",
                "g",
                code,
                sjis
            ),
            "[spoiler]>>12[/spoiler] [code]>>34[/code]"
        );
    }
    assert_eq!(
        prepare(">>>>/g/1 >>>/g/>>>/g/2", "g", false, false),
        ">>>1 >>>/g/>>2"
    );
    let unchanged = ">>>/gg/1 >>>/G/1 >>>/g/ >>>/g/-1 >>>/g/+1 >>>/g/one >>>/../1 >>>/g?x/1";
    assert_eq!(prepare(unchanged, "g", false, false), unchanged);
    for board in ["", "..", "G", "g/", "g|test", "abcdefghijk"] {
        let text = format!(">>>/{board}/1");
        assert_eq!(prepare(&text, board, false, false), text);
    }
}

#[test]
fn unicode_rewrite_order_and_board_exceptions_match_the_source() {
    assert_eq!(prepare("＞＞＞/ｇ/１２", "g", true, false), ">>12");
    assert_eq!(prepare(">>>/g/\u{200b}1", "g", false, false), ">>1");
    assert_eq!(prepare(">>>/g/\u{31350}1", "g", false, false), ">>>/g/1");
    assert_eq!(prepare(">>>/g/１", "g", false, true), ">>>/g/１");
    assert_eq!(
        prepare(">>>/a/１ >>>/a/1", "a", false, false),
        ">>>/a/１ >>1"
    );
    assert_eq!(
        prepare(">>>/jp/１ >>>/jp/1", "jp", false, false),
        ">>>/jp/１ >>1"
    );
    // Arabic-Indic digits are outside both the finite ASCII map and [0-9].
    assert_eq!(prepare(">>>/g/١", "g", false, false), ">>>/g/١");
}

#[test]
fn shortening_cannot_evade_input_limits_and_long_digit_strings_stay_bounded() {
    let policy = CommentSpacing::for_board("g", false, false);
    assert!(prepare_post_comment("", "", ">>>/g/1", 6, false, policy).is_err());
    assert_eq!(
        prepare_post_comment("", "", ">>>/g/1", 7, false, policy).unwrap(),
        ">>1"
    );
    let digits = "9".repeat(15994);
    assert_eq!(
        prepare(&format!(">>>/g/{digits}"), "g", false, false),
        format!(">>{digits}")
    );
    let raw = ">>>/g/x>>>/g/0".repeat(1000);
    assert_eq!(prepare(&raw, "g", false, false), ">>>/g/x>>0".repeat(1000));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn repeated_literal_rewrites_preserve_every_digit_and_other_board(
        board in "[a-z][a-z0-9]{0,8}", digits in "[0-9]{1,100}", repeats in 1usize..32,
        code in any::<bool>(), sjis in any::<bool>()
    ) {
        let other = format!("{board}x");
        let input = format!(">>>/{board}/{digits}!>>>/{other}/{digits} ").repeat(repeats);
        let expected = format!(">>{digits}!>>>/{other}/{digits} ").repeat(repeats);
        let actual = prepare(&input, &board, code, sjis);
        prop_assert_eq!(&actual, expected.trim_end());
        prop_assert!(actual.len() <= input.len());
    }
}
