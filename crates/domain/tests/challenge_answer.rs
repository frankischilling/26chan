use board_domain::challenge_secrets::{TokenKind, VerificationKey, VerificationToken};
use serde_json::Value;

#[test]
fn answer_digests_match_pinned_primary_twister_normalization_with_explicit_bounds() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../fixtures/challenge-answer-reference.json"
    ))
    .unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 20);
    let key = VerificationKey::parse(&"13".repeat(32)).unwrap();
    let token = VerificationToken::generate(TokenKind::Challenge).unwrap();
    let other = VerificationToken::generate(TokenKind::Challenge).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let raw = case["raw"].as_str().unwrap();
        let normalized = case["normalized"].as_str().unwrap();
        let actual = key.answer_hash(&token, raw);
        if case["within_rust_bounds"].as_bool().unwrap() {
            assert!(actual.is_some(), "vector {index}");
            assert_eq!(
                actual,
                key.answer_hash(&token, normalized),
                "vector {index}"
            );
            assert_ne!(
                actual,
                key.answer_hash(&other, normalized),
                "challenge binding {index}"
            );
            let wrong = if normalized == "AAAAAA" {
                "GGGGGG"
            } else {
                "AAAAAA"
            };
            assert_ne!(
                actual,
                key.answer_hash(&token, wrong),
                "wrong answer {index}"
            );
        } else {
            assert!(actual.is_none(), "bounded rejection {index}");
        }
    }
}
