use board_domain::comment_markup::MarkupPolicy;

#[test]
fn only_known_post_format_versions_grant_source_markup() {
    for value in i16::MIN..=i16::MAX {
        let policy = MarkupPolicy::from_post_format(value);
        if matches!(value, 8..=15 | 24..=31 | 40..=47 | 56..=63 | 104..=111 | 120..=127 | 232..=239 | 248..=255)
        {
            let policy = policy.unwrap();
            assert_eq!(policy.spoilers, value & 1 != 0);
            assert_eq!(policy.code, value & 2 != 0);
            assert_eq!(policy.sjis, value & 4 != 0);
            assert_eq!(policy.op, value & 16 != 0);
        } else {
            assert_eq!(policy, None, "{value}");
        }
    }
}

#[test]
fn source_greentext_pass_matches_exact_prefix_and_wrapping_vectors() {
    use board_domain::{
        filtered_formatting, parse_post_comment_on_board, wordfilter::Profile, wordfiltered_comment,
    };
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/greentext.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 64);
    for case in cases {
        let input = case["input"].as_str().unwrap();
        let expected = case["output"].as_str().unwrap();
        for format in (232..=239).chain(248..=255) {
            let lines = parse_post_comment_on_board(input, format, "g");
            assert_eq!(
                filtered_formatting::source_projection(&lines),
                expected,
                "unfiltered {format}: {input:?}"
            );
            let mut prepared = wordfiltered_comment::prepare(
                input,
                MarkupPolicy::from_post_format(format).unwrap(),
                Profile::Global,
                None,
            )
            .unwrap();
            prepared.freeze_format("g");
            let bytes = prepared.encode().unwrap();
            let decoded = wordfiltered_comment::PreparedComment::decode(&bytes).unwrap();
            let lines = filtered_formatting::lines_for_post(&decoded, "g", format);
            assert_eq!(
                filtered_formatting::source_projection(&lines),
                expected,
                "filtered {format}: {input:?}"
            );
        }
    }
}

#[test]
fn corrected_greentext_does_not_reinterpret_historic_profiles() {
    use board_domain::{filtered_formatting, parse_post_comment_on_board};
    for format in [8, 15, 24, 31, 40, 47, 56, 63, 104, 111, 120, 127] {
        let lines = parse_post_comment_on_board(">>not-a-post", format, "g");
        assert_eq!(
            filtered_formatting::source_projection(&lines),
            "&gt;&gt;not-a-post"
        );
    }
    let lines = parse_post_comment_on_board(">>not-a-post", 232, "g");
    assert_eq!(
        filtered_formatting::source_projection(&lines),
        "<span class=\"quote\">&gt;&gt;not-a-post</span>"
    );
}
