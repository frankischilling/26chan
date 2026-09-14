use board_domain::{validate_post, validate_post_with_attachment};

#[test]
fn attachment_request_allows_empty_or_whitespace_comments_without_granting_authority() {
    assert!(validate_post("", "", "", 2000).is_err());
    assert!(validate_post_with_attachment("", "", "", 2000, false).is_err());
    assert!(validate_post_with_attachment("", "", "", 2000, true).is_ok());
    for text in [" ", "\t\r\n", "\u{a0}", "\u{2003}"] {
        assert!(validate_post_with_attachment("", "", text, 2000, true).is_ok());
        assert!(validate_post_with_attachment("", "", text, 2000, false).is_err());
    }
    for attached in [false, true] {
        assert!(validate_post_with_attachment("", "", "hello", 5, attached).is_ok());
        assert!(validate_post_with_attachment("", "", "hello!", 5, attached).is_err());
        assert!(validate_post_with_attachment("", "", "\0", 2000, attached).is_err());
        assert!(
            validate_post_with_attachment("", "", &"x".repeat(16001), 16000, attached).is_err()
        );
    }
    assert!(
        validate_post_with_attachment(&"n".repeat(100), &"s".repeat(100), "", 2000, true).is_ok()
    );
    assert!(validate_post_with_attachment(&"n".repeat(101), "", "", 2000, true).is_err());
    assert!(validate_post_with_attachment("", &"s".repeat(101), "", 2000, true).is_err());
    assert!(validate_post_with_attachment("\0", "", "", 2000, true).is_err());
    assert!(validate_post_with_attachment("", "\0", "", 2000, true).is_err());
}
