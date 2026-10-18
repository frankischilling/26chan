use super::*;
use ProviderValue::{Available as A, Unavailable as U};
fn result(request: ProviderValue<Request<'_>>) -> Option<f64> {
    match score(request) {
        A(value) => Some(value.value()),
        U => None,
    }
}
fn empty() -> Request<'static> {
    Request {
        server: &[],
        cookies: &[],
        challenge: None,
        response: None,
        request_time: 1000,
        country: None,
        is_op: false,
        multipart: true,
    }
}
#[test]
fn numeric_score_matches_isolated_source_cases() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/threat-score.json")).unwrap();
    assert_eq!(
        fixture["source_sha256"],
        "d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207"
    );
    assert_eq!(
        fixture["function_sha256"],
        "258a972c331faab651eff2f0ee2444553b6ff01ffa5b7238b4fd7920b87c1503"
    );
    assert_eq!(
        fixture["language_sha256"],
        "31af597ad24fb84c5e19d3b945d83467d986802e3acd78b34fcbbbf9094f41cb"
    );
    assert_eq!(fixture["php_int_size"], 8);
    assert_eq!(fixture["php_version"], "8.4.24");
    assert_eq!(fixture["pcre_version"], "10.46 2025-08-27");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4524);
    fn entries(value: &serde_json::Value) -> Vec<(&str, &[u8])> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v[0].as_str().unwrap(), v[1].as_str().unwrap().as_bytes()))
            .collect()
    }
    for case in cases {
        let server = entries(&case["server"]);
        let cookies = entries(&case["cookies"]);
        let optional = |key: &str| case[key].as_str().map(str::as_bytes);
        let actual = result(A(Request {
            server: &server,
            cookies: &cookies,
            challenge: optional("challenge"),
            response: optional("response"),
            country: optional("country"),
            request_time: case["time"].as_i64().unwrap(),
            is_op: case["op"].as_bool().unwrap(),
            multipart: case["multipart"].as_bool().unwrap(),
        }));
        assert_eq!(actual, case["score"].as_f64(), "{}", case["name"]);
    }
}
#[test]
fn qualification_and_bounds_never_invent_a_low_score() {
    assert_eq!(result(U), None);
    let huge = vec![b'x'; 4097];
    assert_eq!(
        result(A(Request {
            server: &[("HTTP_USER_AGENT", &huge)],
            ..empty()
        })),
        None
    );
    assert_eq!(
        result(A(Request {
            server: &[("HTTP_ACCEPT", b"a"), ("HTTP_ACCEPT", b"b")],
            ..empty()
        })),
        None
    );
    assert_eq!(
        result(A(Request {
            cookies: &[("_tcs", b"a"), ("_tcs", b"b")],
            ..empty()
        })),
        None
    );
    assert_eq!(
        result(A(Request {
            request_time: i64::MAX,
            ..empty()
        })),
        None
    );
    assert_eq!(
        result(A(Request {
            response: Some(&huge),
            ..empty()
        })),
        None
    );
}
#[test]
fn rounding_and_numeric_coercion_preserve_boundaries() {
    for (number, expected) in [
        (0.145, 0.15),
        (0.285, 0.29),
        (0.305, 0.31),
        (0.395, 0.4),
        (1.005, 1.01),
        (1.0049, 1.0),
    ] {
        assert_eq!(round_two(number), expected);
    }
    for value in [b"33".as_slice(), b"033", b" 33\t", b"+33", b"3.3e1"] {
        assert_eq!(numeric(value), Some(33.0));
    }
    for value in [b"".as_slice(), b"junk", b"NaN", b"inf", b"33junk", b"1e999"] {
        assert_eq!(numeric(value), None);
    }
}

proptest::proptest! {
    #[test]
    fn bounded_arbitrary_bytes_never_panic_or_produce_nonfinite_scores(
        ua in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512),
        language in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128),
        cookie in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128),
    ) {
        let server = [("HTTP_USER_AGENT", ua.as_slice()), ("HTTP_ACCEPT_LANGUAGE", language.as_slice())];
        let cookies = [("_tcs", cookie.as_slice())];
        if let Some(value) = result(A(Request { server: &server, cookies: &cookies, challenge: Some(b"owned"), response: Some(b"answer"), country: Some(b"JP"), ..empty() })) {
            proptest::prop_assert!(value.is_finite() && value >= 0.0);
        }
    }
}

#[test]
fn native_origin_requires_evidence_only_when_referer_is_present() {
    let value = |score: ProviderValue<Score>| match score {
        A(value) => Some(value.value()),
        U => None,
    };
    assert_eq!(
        value(score_for_native_origin(A(empty()), U)),
        result(A(empty()))
    );
    assert!(matches!(score_for_native_origin(U, A(true)), U));
    let fields = [("HTTP_REFERER", b"https://owned.example/g/".as_slice())];
    let request = Request {
        server: &fields,
        ..empty()
    };
    assert!(matches!(score_for_native_origin(A(request), U), U));
    assert_eq!(
        value(score_for_native_origin(A(request), A(false))),
        result(A(request))
    );
    let A(allowed) = score_for_native_origin(A(request), A(true)) else {
        panic!("qualified origin");
    };
    assert!(allowed.value() < result(A(request)).unwrap());
    let duplicate = [fields[0], fields[0]];
    assert!(matches!(
        score_for_native_origin(
            A(Request {
                server: &duplicate,
                ..empty()
            }),
            A(true)
        ),
        U
    ));
}

#[test]
fn native_report_threshold_is_inclusive_and_posting_inputs_cannot_mint_it() {
    assert!(!NativeReportScore(Score(0.399999999)).at_least_point_four());
    assert!(NativeReportScore(Score(0.4)).at_least_point_four());
    assert!(NativeReportScore(Score(0.400000001)).at_least_point_four());
    let request = Request {
        server: &[],
        cookies: &[],
        challenge: None,
        response: None,
        request_time: 1000,
        country: None,
        is_op: true,
        multipart: false,
    };
    assert!(matches!(
        score_native_report(
            ProviderValue::Available(request),
            ProviderValue::Available(false)
        ),
        ProviderValue::Available(_)
    ));
    for invalid in [
        Request {
            is_op: false,
            ..request
        },
        Request {
            multipart: true,
            ..request
        },
        Request {
            country: Some(b"US"),
            ..request
        },
    ] {
        assert!(matches!(
            score_native_report(
                ProviderValue::Available(invalid),
                ProviderValue::Available(false)
            ),
            ProviderValue::Unavailable
        ));
    }
}
