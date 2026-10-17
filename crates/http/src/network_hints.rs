//! HTTP-to-domain adapter for non-authoritative abuse-policy hints.
use crate::transport::RequestHeaderOrder;
use axum::http::HeaderMap;
use board_domain::content_admission::network::hints::{BrowserHeaders, BrowserKey, DerivedHints};

pub fn derive(
    headers: &HeaderMap,
    order: Option<&RequestHeaderOrder>,
    key: Option<&BrowserKey>,
) -> DerivedHints {
    let browser = browser_headers(headers);
    let names: Option<Vec<_>> =
        order.map(|order| order.names().map(|name| name.as_str()).collect());
    DerivedHints::derive(key, browser, names.as_deref())
}

fn browser_headers(headers: &HeaderMap) -> Option<BrowserHeaders<'_>> {
    let mut values = [None; 4];
    let mut ambiguous = false;
    for (name, value) in headers {
        let index = match name.as_str().replace('_', "-").as_str() {
            "user-agent" => 0,
            "sec-ch-ua-mobile" => 1,
            "sec-ch-ua-model" => 2,
            "sec-fetch-mode" => 3,
            _ => continue,
        };
        if values[index].replace(value.as_bytes()).is_some() {
            ambiguous = true;
        }
    }
    // Duplicate browser-value fields have SAPI-specific joining rules. Refuse
    // that provider rather than choosing an arbitrary HeaderMap value/order.
    (!ambiguous).then_some(BrowserHeaders {
        user_agent: values[0],
        mobile: values[1],
        model: values[2],
        fetch_mode: values[3],
    })
}

/// Separate namespace used only for the historical thread-specific classifier.
pub fn posting_browser_match(
    headers: &HeaderMap,
    key: Option<&board_domain::content_admission::classifiers::posting::browser::CompatibilityKey>,
) -> board_domain::content_admission::network::ProviderValue<
    board_domain::content_admission::classifiers::posting::browser::Match,
> {
    // The source falsey-UA result precedes all other browser fields and the key.
    let mut user_agent = None;
    for (name, value) in headers {
        if name.as_str().replace('_', "-") == "user-agent"
            && user_agent.replace(value.as_bytes()).is_some()
        {
            return board_domain::content_admission::network::ProviderValue::Unavailable;
        }
    }
    if user_agent.is_none_or(|ua| ua.is_empty() || ua == b"0") {
        return board_domain::content_admission::classifiers::posting::browser::classify(
            None,
            Some(BrowserHeaders {
                user_agent,
                ..Default::default()
            }),
        );
    }
    board_domain::content_admission::classifiers::posting::browser::classify(
        key,
        browser_headers(headers),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_domain::content_admission::network::ProviderValue;
    #[test]
    fn no_header_map_order_fallback_and_ambiguous_browser_fields_are_unavailable() {
        let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "Firefox/123.4".parse().unwrap());
        let hints = derive(&headers, None, Some(&key));
        assert!(matches!(
            hints.signatures().browser_id,
            ProviderValue::Available(Some(_))
        ));
        assert!(matches!(
            hints.signatures().request_signature,
            ProviderValue::Unavailable
        ));
        headers.insert("user_agent", "Different/1".parse().unwrap());
        assert!(matches!(
            derive(&headers, None, Some(&key)).signatures().browser_id,
            ProviderValue::Unavailable
        ));
        headers.remove("user_agent");
        headers.append("user-agent", "Repeated/2".parse().unwrap());
        assert!(matches!(
            derive(&headers, None, Some(&key)).signatures().browser_id,
            ProviderValue::Unavailable
        ));
    }

    #[test]
    fn rule_only_browser_observations_do_not_use_the_operator_hint_key() {
        use board_domain::content_admission::{
            classifiers::posting::browser::CompatibilityKey,
            network::ProviderValue::{Available as A, Unavailable as U},
        };
        let operator = BrowserKey::parse(&"11".repeat(32)).unwrap();
        let compat = CompatibilityKey::parse(&"22".repeat(64)).unwrap();
        let mut headers = HeaderMap::new();
        assert!(matches!(posting_browser_match(&headers, None), A(_)));
        headers.append("sec-ch-ua-mobile", "?0".parse().unwrap());
        headers.append("sec-ch-ua-mobile", "?1".parse().unwrap());
        assert!(matches!(posting_browser_match(&headers, None), A(_)));
        headers.remove("sec-ch-ua-mobile");
        headers.insert("user-agent", "Firefox/123".parse().unwrap());
        assert!(matches!(
            derive(&headers, None, Some(&operator))
                .signatures()
                .browser_id,
            A(Some(_))
        ));
        assert!(matches!(posting_browser_match(&headers, None), U));
        assert!(matches!(
            posting_browser_match(&headers, Some(&compat)),
            A(_)
        ));
        headers.append("user-agent", "Repeated/2".parse().unwrap());
        assert!(matches!(posting_browser_match(&headers, Some(&compat)), U));
        headers.remove("user-agent");
        headers.insert("user-agent", "Firefox/123".parse().unwrap());
        headers.insert("user_agent", "Alias/1".parse().unwrap());
        assert!(matches!(posting_browser_match(&headers, Some(&compat)), U));
    }
}
