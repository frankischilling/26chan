use board_domain::comment_markup::MarkupPolicy;

#[test]
fn only_known_post_format_versions_grant_source_markup() {
    for value in i16::MIN..=i16::MAX {
        let policy = MarkupPolicy::from_post_format(value);
        if matches!(value, 8..=15 | 24..=31) {
            let policy = policy.unwrap();
            assert_eq!(
                policy.spoilers,
                matches!(value, 9 | 11 | 13 | 15 | 25 | 27 | 29 | 31)
            );
            assert_eq!(
                policy.code,
                matches!(value, 10 | 11 | 14 | 15 | 26 | 27 | 30 | 31)
            );
            assert_eq!(policy.sjis, matches!(value, 12..=15 | 28..=31));
            assert_eq!(policy.op, matches!(value, 24..=31));
        } else {
            assert_eq!(policy, None, "{value}");
        }
    }
}
