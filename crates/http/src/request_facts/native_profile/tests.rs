use super::super::capture_ordered;
use super::*;
use ProviderValue::{Available as A, Unavailable as U};
use axum::http::HeaderMap;

fn context() -> Context<'static> {
    Context {
        request_time: A(1000),
        challenge: None,
        response: None,
        mode: Mode::Reporting,
    }
}
fn captured(fields: &[(&str, &str)]) -> CapturedRequest {
    let mut headers = HeaderMap::new();
    for (key, value) in fields {
        headers.insert(
            axum::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    capture_ordered(
        &headers,
        &fields.iter().map(|(key, _)| *key).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn number(value: ProviderValue<threat::Score>) -> f64 {
    let A(value) = value else {
        panic!("Qualified native profile must have a score");
    };
    value.value()
}
#[test]
fn unavailable_facts_do_not_produce_a_low_score() {
    let origin = Origin::parse("https://board.example").unwrap();
    let clean = captured(&[("user-agent", "owned")]);
    assert!(matches!(score(U, &origin, context()), U));
    let mut input = context();
    input.request_time = U;
    assert!(matches!(score(A(&clean), &origin, input), U));
    for country in [U, A(&b"us"[..]), A(&b"USA"[..]), A(&b""[..])] {
        let mut input = context();
        input.mode = Mode::Posting {
            is_op: false,
            country,
        };
        assert!(matches!(score(A(&clean), &origin, input), U));
    }
    let bad = captured(&[("cookie", "_tcs=x%2eUTC")]);
    assert!(matches!(score(A(&bad), &origin, context()), U));
    let mut input = context();
    input.request_time = A(i64::MAX);
    assert!(matches!(score(A(&clean), &origin, input), U));
}
#[test]
fn native_origin_check_does_not_rewrite_observed_fields() {
    let origin = Origin::parse("https://board.example").unwrap();
    let base = captured(&[
        ("user-agent", "Mozilla/5.0 (Windows NT 10.0) Firefox/120.0"),
        ("referer", "https://board.example/g/"),
    ]);
    let expected = number(score(A(&base), &origin, context()));
    for same in ["https://BOARD.example:443/g/?q=1", "https://board.example/"] {
        let observed = captured(&[
            ("user-agent", "Mozilla/5.0 (Windows NT 10.0) Firefox/120.0"),
            ("referer", same),
        ]);
        assert_eq!(number(score(A(&observed), &origin, context())), expected);
        assert_eq!(observed.http_fields()[1].1, same.as_bytes());
    }
    for foreign in [
        "http://board.example/",
        "https://board.example:444/",
        "blob:https://board.example/owned",
        "https://other.example/4chan.org",
        "https://board.example.other/",
        "https://name@board.example/",
        "https://board.example@other.example/",
        "/g/",
        "",
        " https://board.example/",
    ] {
        let observed = captured(&[
            ("user-agent", "Mozilla/5.0 (Windows NT 10.0) Firefox/120.0"),
            ("referer", foreign),
        ]);
        assert!(
            number(score(A(&observed), &origin, context())) > expected,
            "{foreign}"
        );
    }
}
#[test]
fn native_profile_retains_order_and_mode_without_server_aliases() {
    let origin = Origin::parse("https://board.example").unwrap();
    let base = [
        (
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36",
        ),
        ("sec-fetch-site", "same-origin"),
        ("sec-fetch-mode", "cors"),
        ("accept", "text/html"),
    ];
    let together = captured(&base);
    let separated = captured(&[base[0], base[1], base[3], base[2]]);
    assert!(
        number(score(A(&separated), &origin, context()))
            > number(score(A(&together), &origin, context()))
    );
    assert!(
        together
            .http_fields()
            .iter()
            .all(|(key, _)| key.starts_with("HTTP_"))
    );
}

#[test]
fn compatible_native_profiles_match_executed_php_cases() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../domain/tests/fixtures/threat-score.json"
    ))
    .unwrap();
    let origin = Origin::parse("https://boards.4chan.org").unwrap();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let server = case["server"].as_array().unwrap();
        let mut headers = HeaderMap::new();
        let mut names = Vec::new();
        let mut applicable = true;
        for item in server {
            let key = item[0].as_str().unwrap();
            let value = item[1].as_str().unwrap();
            let Some(key) = key.strip_prefix("HTTP_") else {
                applicable = false;
                break;
            };
            let key = key.replace('_', "-").to_ascii_lowercase();
            let Ok(name) = axum::http::HeaderName::from_bytes(key.as_bytes()) else {
                applicable = false;
                break;
            };
            let Ok(value_header) = value.parse::<axum::http::HeaderValue>() else {
                applicable = false;
                break;
            };
            if key == "referer" && value.contains("4chan.org") != origin.matches_url(value) {
                applicable = false;
                break;
            }
            headers.insert(name, value_header);
            names.push(key);
        }
        if !applicable {
            continue;
        }
        let borrowed: Vec<_> = names.iter().map(String::as_str).collect();
        let Some(observed) = capture_ordered(&headers, &borrowed) else {
            continue;
        };
        let A(cookies) = observed.cookies() else {
            continue;
        };
        let expected_cookies: Vec<_> = case["cookies"]
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
        if cookies != expected_cookies {
            continue;
        }
        // Only actual source caller modes are admitted by this native profile.
        let mode = if case["multipart"].as_bool().unwrap() {
            let Some(country) = case["country"].as_str() else {
                continue;
            };
            if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
                continue;
            }
            Mode::Posting {
                is_op: case["op"].as_bool().unwrap(),
                country: A(country.as_bytes()),
            }
        } else if case["op"] == true && case["country"].is_null() {
            Mode::Reporting
        } else {
            continue;
        };
        let result = score(
            A(&observed),
            &origin,
            Context {
                request_time: A(case["time"].as_i64().unwrap()),
                challenge: case["challenge"].as_str().map(str::as_bytes),
                response: case["response"].as_str().map(str::as_bytes),
                mode,
            },
        );
        if case["score"].is_null() {
            assert!(matches!(result, U), "{}", case["name"]);
        } else {
            assert_eq!(
                number(result),
                case["score"].as_f64().unwrap(),
                "{}",
                case["name"]
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 2797, "Qualified native-profile subset changed");
}
