use board_domain::comment_markup::MarkupPolicy;

#[test]
fn only_known_post_format_versions_grant_source_markup() {
    for value in i16::MIN..=i16::MAX {
        let policy = MarkupPolicy::from_post_format(value);
        if matches!(value, 8..=15 | 24..=31 | 40..=47 | 56..=63) {
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
