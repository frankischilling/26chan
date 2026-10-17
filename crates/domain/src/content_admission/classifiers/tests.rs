use super::*;
use ProviderValue::{Available as A, Unavailable as U};

fn decision(request: CreditRequest<'_>) -> Option<bool> {
    match withhold_captcha_credit(request) {
        A(value) => Some(value),
        U => None,
    }
}

fn request() -> CreditRequest<'static> {
    CreditRequest {
        accept_language: A(Some(b"en")),
        accept: A(Some(b"text/html")),
        user_agent: A(Some(b"Mozilla/5.0")),
        referer: A(None),
    }
}
#[test]
fn credit_predicate_preserves_header_presence_and_source_case() {
    let mut r = request();
    assert_eq!(decision(r), Some(false));
    r.accept_language = A(None);
    r.accept = A(None);
    assert_eq!(decision(r), Some(true));
    r.accept_language = A(Some(b""));
    r.accept = A(Some(b""));
    assert_eq!(decision(r), Some(false));
    r.accept_language = A(None);
    r.user_agent = A(Some(b"Android; wv)"));
    assert_eq!(decision(r), Some(true));
    r.user_agent = A(Some(b"Android; WV)"));
    assert_eq!(decision(r), Some(false));
    r.referer = A(Some(b"https://example.test/thread/1"));
    assert_eq!(decision(r), Some(true));
}
#[test]
fn independent_facts_do_not_turn_unknowns_into_credit_eligibility() {
    let mut r = request();
    r.user_agent = U;
    r.referer = U;
    // Both presence facts make every source branch false without these values.
    assert_eq!(decision(r), Some(false));
    r.accept = A(None);
    assert_eq!(decision(r), None);
    r.accept_language = A(None);
    assert_eq!(decision(r), Some(true));
    r.accept = A(Some(b"text/html"));
    r.user_agent = A(Some(b"Android; wv)"));
    assert_eq!(decision(r), Some(true));
    r.user_agent = A(Some(b"iPhone"));
    assert_eq!(decision(r), None);
}
#[test]
fn iphone_exclusion_and_byte_bound_are_explicit() {
    let mut r = request();
    r.accept = A(None);
    r.user_agent = A(Some(b"iPhone"));
    assert_eq!(decision(r), Some(true));
    r.user_agent = A(Some(b"iPhone Mobile"));
    assert_eq!(decision(r), Some(false));
    let oversized = vec![b'x'; MAX_HEADER_BYTES + 1];
    r.user_agent = A(Some(&oversized));
    assert_eq!(decision(r), None);
}

#[test]
fn credit_predicate_matches_isolated_source_requests() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/credit-request.json")).unwrap();
    assert_eq!(
        corpus["source_sha256"],
        "d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207"
    );
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 450);
    fn fact(value: &serde_json::Value) -> ProviderValue<Option<&[u8]>> {
        assert!(value.is_null() || value.is_string());
        A(value.as_str().map(str::as_bytes))
    }
    for (index, case) in cases.iter().enumerate() {
        let request = CreditRequest {
            accept_language: fact(&case["language"]),
            accept: fact(&case["accept"]),
            user_agent: fact(&case["ua"]),
            referer: fact(&case["referer"]),
        };
        assert_eq!(
            decision(request),
            Some(case["withhold"].as_bool().unwrap()),
            "source case {index}"
        );
    }
}

#[test]
fn automation_requires_the_trusted_score_and_preserves_webview_exclusions() {
    fn outcome(r: AutomationRequest<'_>) -> Option<bool> {
        match likely_automated(r) {
            A(value) => Some(value),
            U => None,
        }
    }
    let mut r = AutomationRequest {
        edge_score: A(None),
        user_agent: U,
        cached_marker: A(true),
    };
    assert_eq!(outcome(r), Some(false));
    r.edge_score = A(Some(29));
    assert_eq!(outcome(r), None);
    r.user_agent = A(Some(b"Mozilla/5.0"));
    assert_eq!(outcome(r), Some(true));
    r.cached_marker = U;
    assert_eq!(outcome(r), Some(true));
    r.edge_score = A(Some(30));
    assert_eq!(outcome(r), None);
    r.cached_marker = A(false);
    assert_eq!(outcome(r), Some(false));
    r.edge_score = A(Some(1));
    r.user_agent = A(Some(b"iPad"));
    assert_eq!(outcome(r), Some(false));
    r.user_agent = A(Some(b"iPad Safari"));
    assert_eq!(outcome(r), Some(true));
    r.user_agent = A(Some(b"Android; wv)"));
    assert_eq!(outcome(r), Some(false));
    r.edge_score = U;
    assert_eq!(outcome(r), Some(false));
    r.user_agent = A(Some(b"Mozilla/5.0"));
    assert_eq!(outcome(r), None);
}

#[test]
fn automation_predicate_matches_source_score_and_cache_matrix() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/automation-request.json"
    ))
    .unwrap();
    assert_eq!(
        corpus["source_sha256"],
        "d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207"
    );
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 429);
    for (index, case) in cases.iter().enumerate() {
        assert!(case["score"].is_null() || case["score"].is_i64());
        assert!(case["ua"].is_null() || case["ua"].is_string());
        let request = AutomationRequest {
            edge_score: A(case["score"].as_i64()),
            user_agent: A(case["ua"].as_str().map(str::as_bytes)),
            cached_marker: A(if case["cached"].is_null() {
                false
            } else {
                case["cached"].as_bool().unwrap()
            }),
        };
        let actual = match likely_automated(request) {
            A(value) => value,
            U => panic!("complete source case became unavailable"),
        };
        assert_eq!(
            actual,
            case["automated"].as_bool().unwrap(),
            "source case {index}"
        );
    }
}
