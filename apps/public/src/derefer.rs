//! Bounded project redirect page; no destination is fetched by this service.
use crate::AppState;
use askama::Template;
use axum::{
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use url::Url;

// Keep the derefer boundary aligned with the browser linkifier, whose bound is
// measured in JavaScript UTF-16 code units. A BMP scalar can take three UTF-8
// bytes and nine percent-encoded query bytes per UTF-16 unit.
const MAX_URL_UTF16: usize = 192_000;
const MAX_QUERY_BYTES: usize = MAX_URL_UTF16 * 9 + 32;

#[derive(Template)]
#[template(path = "derefer.html")]
struct Page {
    destination: String,
    domain: String,
}

pub async fn get(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    // noreferrer links omit their referrer. A supplied referrer must match the
    // configured public origin exactly.
    if headers.get_all(header::REFERER).iter().count() > 1 {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(value) = headers.get(header::REFERER) {
        let valid = value.to_str().ok().is_some_and(|raw| {
            raw.is_empty()
                || (raw.len() <= 8192
                    && valid_url(raw)
                        .is_some_and(|url| url.origin().ascii_serialization() == state.origin))
        });
        if !valid {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    let Some(query) = query else {
        return StatusCode::OK.into_response();
    };
    if query.len() > MAX_QUERY_BYTES {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    if !strict_form_encoding(&query) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut value = None;
    for (key, input) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "url" {
            if value.is_some() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            value = Some(input.into_owned());
        }
    }
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return StatusCode::OK.into_response();
    };
    if value.encode_utf16().count() > MAX_URL_UTF16 {
        return StatusCode::URI_TOO_LONG.into_response();
    }
    let destination = decode_special_entities(&value);
    let Some(url) = valid_url(&destination) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Display a canonical host so the shown authority agrees with browser URL
    // interpretation, including internationalized names and numeric addresses.
    let page = Page {
        domain: url.host_str().expect("validated host").into(),
        destination,
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn valid_url(raw: &str) -> Option<Url> {
    if raw.bytes().any(|b| b <= b' ' || b == 127 || b == b'\\') {
        return None;
    }
    let (scheme, _) = raw.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (url.has_host() && url.username().is_empty() && url.password().is_none()).then_some(url)
}

fn strict_form_encoding(raw: &str) -> bool {
    fn hex(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }

    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some((high, low)) = bytes
                .get(index + 1)
                .and_then(|value| hex(*value))
                .zip(bytes.get(index + 2).and_then(|value| hex(*value)))
            else {
                return false;
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    std::str::from_utf8(&decoded).is_ok()
}

// ENT_QUOTES with HTML401: four named entities and numeric spellings of only
// &, <, >, single quote and double quote. Decode one pass, never recursively.
fn decode_special_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut tail = input;
    while let Some(index) = tail.find('&') {
        output.push_str(&tail[..index]);
        tail = &tail[index + 1..];
        let len = tail
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'#')
            .count();
        let entity = &tail[..len];
        let decoded = if tail.as_bytes().get(len) != Some(&b';') {
            None
        } else {
            match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                _ => entity.strip_prefix('#').and_then(|number| {
                    let (digits, radix) = number
                        .strip_prefix(['x', 'X'])
                        .map_or((number, 10), |hex| (hex, 16));
                    if digits.is_empty()
                        || !digits.bytes().all(|b| {
                            if radix == 16 {
                                b.is_ascii_hexdigit()
                            } else {
                                b.is_ascii_digit()
                            }
                        })
                    {
                        return None;
                    }
                    let digits = digits.trim_start_matches('0');
                    u32::from_str_radix(digits, radix)
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|ch| matches!(ch, '&' | '<' | '>' | '\'' | '"'))
                }),
            }
        };
        if let Some(ch) = decoded {
            output.push(ch);
            tail = &tail[len + 1..];
        } else {
            output.push('&');
            output.push_str(entity);
            tail = &tail[len..];
        }
    }
    output.push_str(tail);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[tokio::test]
    async fn independent_handler_bounds_reject_oversized_decoded_and_encoded_queries() {
        for query in [
            format!("url=https://example.test/{}", "x".repeat(MAX_URL_UTF16)),
            format!("url={}", "x".repeat(MAX_QUERY_BYTES)),
        ] {
            let state = AppState {
                pool: sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
                    .unwrap(),
                origin: "https://board.example".into(),
                production: false,
                limits: std::sync::Arc::new(crate::security::Limits::new(
                    board_config::PublicRequestLimits::default(),
                )),
                media: None,
                proxy_uid: None,
            };
            assert_eq!(
                get(State(state), RawQuery(Some(query)), HeaderMap::new())
                    .await
                    .status(),
                StatusCode::URI_TOO_LONG
            );
        }
    }

    #[tokio::test]
    async fn linkifier_utf16_bound_accepts_valid_multibyte_url_and_rejects_one_unit_over() {
        let state = || AppState {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
                .unwrap(),
            origin: "https://board.example".into(),
            production: false,
            limits: std::sync::Arc::new(crate::security::Limits::new(
                board_config::PublicRequestLimits::default(),
            )),
            media: None,
            proxy_uid: None,
        };
        let accepted = format!("https://example.test/{}", "\u{0800}".repeat(70_000));
        assert!(accepted.len() > MAX_URL_UTF16);
        assert!(accepted.encode_utf16().count() < MAX_URL_UTF16);
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", &accepted)
            .finish();
        assert!(query.len() > MAX_URL_UTF16 * 3 + 32);
        assert_eq!(
            get(State(state()), RawQuery(Some(query)), HeaderMap::new())
                .await
                .status(),
            StatusCode::OK
        );

        let over = format!("https://e.test/{}", "x".repeat(MAX_URL_UTF16));
        assert!(over.encode_utf16().count() > MAX_URL_UTF16);
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", &over)
            .finish();
        assert_eq!(
            get(State(state()), RawQuery(Some(query)), HeaderMap::new())
                .await
                .status(),
            StatusCode::URI_TOO_LONG
        );
    }

    #[test]
    fn form_encoding_rejects_malformed_percent_sequences_and_invalid_utf8() {
        for query in [
            "url=https%3A%2F%2Fexample.test%2F%",
            "url=https%3A%2F%2Fexample.test%2F%0g",
            "url=https%3A%2F%2Fexample.test%2F%ff",
            "url=https%3A%2F%2Fexample.test%2F%c3%28",
        ] {
            assert!(!strict_form_encoding(query), "{query}");
        }
        assert!(strict_form_encoding(
            "url=https%3A%2F%2Fexample.test%2F%E0%A0%80%3Fa%3D1%26b%3D2"
        ));
    }

    #[test]
    fn special_entity_decoding_is_single_pass_and_not_a_general_html_parser() {
        assert_eq!(
            decode_special_entities("&amp;&lt;&gt;&quot;&#039;&#x27;&#X22;&#000000038;"),
            "&<>\"''\"&"
        );
        assert_eq!(
            decode_special_entities("&amp;lt;&apos;&nbsp;&#65;&#x41;&AMP;&#-39;&#39 &#x;"),
            "&lt;&apos;&nbsp;&#65;&#x41;&AMP;&#-39;&#39 &#x;"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_entities_remain_bounded(input in ".{0,4096}") {
            let decoded = decode_special_entities(&input);
            prop_assert!(decoded.len() <= input.len());
            if let Some(url) = valid_url(&decoded) {
                prop_assert!(matches!(url.scheme(), "http" | "https"));
                prop_assert!(url.has_host() && url.username().is_empty() && url.password().is_none());
            }
        }
    }
}
