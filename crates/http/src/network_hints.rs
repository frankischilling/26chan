//! HTTP-to-domain adapter for non-authoritative abuse-policy hints.
use crate::transport::RequestHeaderOrder;
use axum::http::HeaderMap;
use board_domain::content_admission::network::hints::{BrowserHeaders, BrowserKey, DerivedHints};

pub fn derive(
    headers: &HeaderMap,
    order: Option<&RequestHeaderOrder>,
    key: Option<&BrowserKey>,
) -> DerivedHints {
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
    let browser = (!ambiguous).then_some(BrowserHeaders {
        user_agent: values[0],
        mobile: values[1],
        model: values[2],
        fetch_mode: values[3],
    });
    let names: Option<Vec<_>> =
        order.map(|order| order.names().map(|name| name.as_str()).collect());
    DerivedHints::derive(key, browser, names.as_deref())
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
}
