use super::*;
use ProviderValue::{Available as A, Unavailable as U};
#[test]
fn parser_order_and_exact_header_map_membership_are_required() {
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "owned".parse().unwrap());
    headers.insert("accept", "text/html".parse().unwrap());
    assert!(matches!(capture(&headers, None), U));
    let observed = capture_ordered(&headers, &["accept", "user-agent"]).unwrap();
    assert_eq!(
        observed.http_fields()[0],
        ("HTTP_ACCEPT".into(), b"text/html".to_vec())
    );
    assert_eq!(
        observed.http_fields()[1],
        ("HTTP_USER_AGENT".into(), b"owned".to_vec())
    );
    assert!(matches!(observed.cookies(), A(values) if values.is_empty()));
    for names in [
        &["accept"][..],
        &["accept", "missing"],
        &["accept", "accept"],
    ] {
        assert!(capture_ordered(&headers, names).is_none());
    }
    headers.append("user-agent", "repeated".parse().unwrap());
    assert!(capture_ordered(&headers, &["accept", "user-agent", "user-agent"]).is_none());
    headers.remove("user-agent");
    headers.insert("user-agent", "owned".parse().unwrap());
    headers.insert("user_agent", "collision".parse().unwrap());
    assert!(capture_ordered(&headers, &["accept", "user-agent", "user_agent"]).is_none());
}
#[test]
fn raw_cookie_and_parsed_cookie_facts_remain_independent() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "cookie",
        "_tcs=x.UTC.999.33; cf_clearance=; __Host-board_session=abc"
            .parse()
            .unwrap(),
    );
    let observed = capture_ordered(&headers, &["cookie"]).unwrap();
    let A(cookies) = observed.cookies() else {
        panic!("canonical subset must parse");
    };
    assert_eq!(cookies.len(), 3);
    assert_eq!(cookies[0], ("_tcs".into(), b"x.UTC.999.33".to_vec()));
    assert_eq!(cookies[1], ("cf_clearance".into(), vec![]));
    headers.insert("cookie", "_tcs=x%2EUTC".parse().unwrap());
    let observed = capture_ordered(&headers, &["cookie"]).unwrap();
    assert!(matches!(observed.cookies(), U));
    assert_eq!(observed.http_fields()[0].1, b"_tcs=x%2EUTC");
}
#[test]
fn ambiguous_or_noncanonical_cookies_are_unavailable() {
    for raw in [
        "a=1; a=2",
        "a.b=1",
        "a b=1",
        "a[]=1",
        "0=1",
        "a=\"quoted\"",
        "a=x+y",
        "a=x%20y",
        "a=x,y",
        "a=x\\y",
        "a=1;",
        ";a=1",
        "a=1;  b=2",
        "a=1\t",
        "a",
        "=1",
        "a=1 ",
    ] {
        assert!(flat_cookies(Some(raw.as_bytes())).is_none(), "{raw:?}");
    }
    assert_eq!(flat_cookies(None), Some(vec![]));
    assert_eq!(flat_cookies(Some(b"")), Some(vec![]));
    assert_eq!(flat_cookies(Some(b"a=")), Some(vec![("a".into(), vec![])]));
}
#[test]
fn capture_bounds_apply_before_copying_values() {
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "x".repeat(4097).parse().unwrap());
    assert!(capture_ordered(&headers, &["user-agent"]).is_none());
    let huge_cookie = vec![b'a'; 4097];
    assert!(flat_cookies(Some(&huge_cookie)).is_none());
    headers.clear();
    headers.insert("cookie", "a=1".parse().unwrap());
    headers.append("cookie", "b=2".parse().unwrap());
    assert!(capture_ordered(&headers, &["cookie", "cookie"]).is_none());
}
#[test]
fn accepted_cookie_subset_matches_actual_php_http_parsing() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/flat-cookies.json")).unwrap();
    assert_eq!(fixture["php_version"], "8.4.24");
    assert_eq!(fixture["php_int_size"], 8);
    assert_eq!(fixture["pcre_version"], "10.46 2025-08-27");
    assert_eq!(fixture["sapi"], "cli-server");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 53);
    for case in cases {
        let raw = case["raw"].as_str().map(str::as_bytes);
        let parsed = flat_cookies(raw);
        if case["accepted_subset"] == true {
            let expected: Vec<_> = case["cookies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    (
                        v[0].as_str().unwrap().to_owned(),
                        v[1].as_str().unwrap().as_bytes().to_vec(),
                    )
                })
                .collect();
            assert_eq!(parsed, Some(expected), "{}", case["raw"]);
        } else {
            assert!(parsed.is_none(), "{}", case["raw"]);
        }
    }
}
#[test]
fn field_count_key_size_and_total_bytes_have_independent_limits() {
    use axum::http::header::HeaderName;
    let mut headers = HeaderMap::new();
    let names: Vec<_> = (0..257).map(|i| format!("x-{i}")).collect();
    for name in &names[..256] {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            "".parse().unwrap(),
        );
    }
    let borrowed: Vec<_> = names[..256].iter().map(String::as_str).collect();
    assert!(capture_ordered(&headers, &borrowed).is_some());
    headers.insert(
        HeaderName::from_bytes(names[256].as_bytes()).unwrap(),
        "".parse().unwrap(),
    );
    assert!(
        capture_ordered(
            &headers,
            &names.iter().map(String::as_str).collect::<Vec<_>>()
        )
        .is_none()
    );
    headers.clear();
    let key = "x".repeat(124); // HTTP_ prefix would exceed 128 bytes.
    headers.insert(
        HeaderName::from_bytes(key.as_bytes()).unwrap(),
        "".parse().unwrap(),
    );
    assert!(capture_ordered(&headers, &[&key]).is_none());
    headers.clear();
    for name in &names[..16] {
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            "x".repeat(4096).parse().unwrap(),
        );
    }
    assert!(
        capture_ordered(
            &headers,
            &names[..16].iter().map(String::as_str).collect::<Vec<_>>()
        )
        .is_none()
    );
}
