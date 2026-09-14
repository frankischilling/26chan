use board_domain::{MAX_COMMENT_BYTES, normalize_comment, validate_post};
use proptest::prelude::*;

#[test]
fn mixed_newlines_are_normalized_before_character_validation() {
    let raw = "é\r\n😀\rb\n<script>";
    let normalized = "é\n😀\nb\n<script>";
    assert_eq!(normalize_comment(raw).unwrap(), normalized);
    let count = normalized.chars().count();
    assert!(validate_post("", "", raw, count).is_ok());
    assert!(validate_post("", "", raw, count - 1).is_err());
    assert!(validate_post("", "", "\r\n\r", 10).is_err());
    assert!(validate_post("", "", "a\r\n\0", 10).is_err());
    assert!(normalize_comment(&"\r\n".repeat(MAX_COMMENT_BYTES / 2 + 1)).is_err());
    assert!(matches!(
        normalize_comment("already\nnormalized").unwrap(),
        std::borrow::Cow::Borrowed(_)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn bounded_unicode_normalization_preserves_content_and_is_idempotent(
        pieces in prop::collection::vec(prop_oneof![Just("\r".to_owned()), Just("\r\n".to_owned()), any::<char>().prop_map(|c| c.to_string())], 0..256)
    ) {
        let raw = pieces.concat();
        let normalized = normalize_comment(&raw).unwrap();
        prop_assert_eq!(normalized.as_ref(), raw.replace("\r\n", "\n").replace('\r', "\n"));
        prop_assert!(!normalized.contains('\r'));
        prop_assert!(normalized.len() <= raw.len());
        let again = normalize_comment(&normalized).unwrap();
        prop_assert_eq!(again.as_ref(), normalized.as_ref());
    }
}
