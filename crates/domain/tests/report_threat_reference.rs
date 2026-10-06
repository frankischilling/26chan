use board_domain::report_threat::{ThreatDecision, ThreatFacts, prove_report_threat};
use board_domain::report_weight::Evidence::{Qualified as Q, Unknown};
use serde_json::{Value, json};

#[test]
fn bounded_positive_proofs_are_supported_by_isolated_full_source_scores() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/report-threat-source.json")).unwrap();
    // These identify the exact source and extracted function executed by the
    // isolated PHP harness. Scores are fixture evidence, not Rust predictions.
    assert_eq!(
        reference["files"]["lib/postfilter.php"],
        "d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207"
    );
    assert_eq!(
        reference["function_sha256"],
        "258a972c331faab651eff2f0ee2444553b6ff01ffa5b7238b4fd7920b87c1503"
    );
    assert_eq!(
        reference["functions"],
        json!(["spam_filter_get_threat_score"])
    );
    assert_eq!(reference["function_lines"], json!([1684, 2316]));
    assert_eq!(reference["call_args"], json!([null, true, false]));
    assert_eq!(reference["constants"], json!({"DEFAULT_BURICHAN": false}));
    assert_eq!(
        reference["country_helper"],
        "fail-closed stub; unreachable with null country"
    );
    assert_eq!(reference["threshold"], json!(0.4));
    assert_eq!(reference["extractor_php"], "8.4.26");
    assert_eq!(reference["extractor_pcre"], "10.46 2025-08-27");
    assert_eq!(
        reference["nonbrowser_regex"],
        "headless|node-fetch|python-|java/|jakarta|-perl|http-?client|-resty-|awesomium/"
    );
    assert_eq!(
        reference["presence_server_keys"],
        json!(["HTTP_PATH", "HTTP_SAME_ORIGIN", "HTTP_REFERRER_POLICY"])
    );

    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 79);
    let mut positives = 0;
    let mut high_but_unknown = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let headers = case["headers"].as_array().unwrap();
        // Test-only qualification matches the harness's complete synthetic
        // lower-case, unique header names. It is not a production HTTP adapter.
        // All fixture values are strings, so presence (even empty) proves isset.
        let header = |wanted: &str| -> Option<&str> {
            headers
                .iter()
                .find(|header| header["name"].as_str() == Some(wanted))
                .map(|header| header["value"].as_str().unwrap())
        };
        let decision = prove_report_threat(ThreatFacts {
            user_agent: Q(header("user-agent").map(str::as_bytes)),
            http_path_is_set: Q(header("path").is_some()),
            http_same_origin_is_set: Q(header("same-origin").is_some()),
            http_referrer_policy_is_set: Q(header("referrer-policy").is_some()),
        });
        let expected_signal = case["expected_signal"].as_bool().unwrap();
        let source_score = case["source_score"].as_f64().unwrap();
        assert!(source_score.is_finite(), "{name}");
        assert_eq!(
            case["source_at_or_above_threshold"],
            json!(source_score >= 0.4),
            "{name}"
        );
        if expected_signal {
            positives += 1;
            assert!(
                matches!(decision, ThreatDecision::AtLeastPointFour { .. }),
                "{name}"
            );
            assert_eq!(decision.weight_evidence(), Q(true), "{name}");
            assert!(source_score >= 0.4, "{name}: {source_score}");
        } else {
            assert_eq!(decision, ThreatDecision::Unknown, "{name}");
            assert_eq!(decision.weight_evidence(), Unknown, "{name}");
            if source_score >= 0.4 {
                high_but_unknown.push(name);
            }
        }
    }
    assert!(positives > 0);
    // Full-source suspicion outside this bounded slice never becomes a false
    // below-threshold proof or an invented numeric result in the Rust API.
    assert_eq!(
        high_but_unknown,
        vec![
            "outside-slice-future-firefox",
            "outside-slice-wrong-report-content-type",
        ]
    );
}
