use board_domain::email_verification::{ALLOWED_DOMAINS, PreparedEmail};
use serde_json::Value;

#[test]
fn prepared_email_matches_every_pinned_primary_source_vector() {
    let reference: Value = serde_json::from_str(include_str!(
        "../../../fixtures/email-verification-reference.json"
    ))
    .unwrap();
    assert_eq!(
        reference["allowed_domains"],
        serde_json::json!(ALLOWED_DOMAINS)
    );
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 18);
    for (index, case) in cases.iter().enumerate() {
        let actual = match PreparedEmail::prepare(case["raw"].as_str().unwrap()) {
            Ok(email) => {
                serde_json::json!({"email":email.recipient(),"identity_email":email.identity()})
            }
            Err(error) => serde_json::json!({"error":error.message()}),
        };
        assert_eq!(actual, case["expected"], "email source vector {index}");
    }
}
