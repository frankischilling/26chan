use board_domain::validate_post;

#[test]
fn public_name_and_subject_limits_count_input_utf8_bytes() {
    for valid in ["x".repeat(100), "é".repeat(50), "😀".repeat(25)] {
        assert!(validate_post(&valid, &valid, "Post", 10).is_ok());
        for (name, subject) in [
            (format!("{valid}x"), valid.clone()),
            (valid.clone(), format!("{valid}x")),
        ] {
            assert!(validate_post(&name, &subject, "Post", 10).is_err());
        }
    }
    assert!(validate_post(&" ".repeat(101), "", "Post", 10).is_err());
    assert!(validate_post("", &" ".repeat(101), "Post", 10).is_err());
    assert!(validate_post("", "", "Post", 10).is_ok());
    assert!(validate_post("A\0", "", "Post", 10).is_err());
}
