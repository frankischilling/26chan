//! Bounded legacy abuse-policy hints. Neither signature is identity or proof
//! of a browser, and neither may authorize posting, deletion or staff access.
use md5::{Digest, Md5};
use ring::hmac;
use std::collections::HashSet;

pub const MAX_USER_AGENT_BYTES: usize = 4_096;
pub const MAX_MODEL_BYTES: usize = 1_024;
pub const MAX_HEADER_NAMES: usize = 128;

pub struct BrowserKey([u8; 32]);

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub &'static str);

impl BrowserKey {
    /// Fresh operator material, never the embedded reference deployment key.
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error("BROWSER_HINT_KEY requires 64 hexadecimal digits."));
        }
        let mut key = [0; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| Error("Invalid BROWSER_HINT_KEY."))?;
        }
        if key.iter().all(|&b| b == 0) {
            return Err(Error("BROWSER_HINT_KEY cannot be all zeroes."));
        }
        Ok(Self(key))
    }
}

#[derive(Default)]
pub struct BrowserHeaders<'a> {
    pub user_agent: Option<&'a [u8]>,
    pub mobile: Option<&'a [u8]>,
    pub model: Option<&'a [u8]>,
    pub fetch_mode: Option<&'a [u8]>,
}

/// Owned derived values only; raw headers and key material are not retained.
#[derive(Clone, Default)]
pub struct DerivedHints {
    browser: Option<String>,
    request: Option<String>,
}

impl DerivedHints {
    pub fn derive(
        key: Option<&BrowserKey>,
        browser: Option<BrowserHeaders<'_>>,
        ordered_names: Option<&[&str]>,
    ) -> Self {
        Self {
            browser: browser.and_then(|headers| browser_id(key, headers).ok()),
            request: ordered_names.and_then(|names| request_signature(names).ok()),
        }
    }
    pub fn signatures(&self) -> super::Signatures<'_> {
        fn value(hint: Option<&str>) -> super::ProviderValue<Option<&str>> {
            match hint {
                Some(value) => super::ProviderValue::Available(Some(value)),
                None => super::ProviderValue::Unavailable,
            }
        }
        super::Signatures {
            browser_id: value(self.browser.as_deref()),
            request_signature: value(self.request.as_deref()),
        }
    }
}

fn bounded(value: Option<&[u8]>, maximum: usize) -> Result<(), Error> {
    if value.is_some_and(|value| {
        value.len() > maximum || value.iter().any(|&b| b == 127 || (b < 32 && b != b'\t'))
    }) {
        return Err(Error("Browser hints are unavailable."));
    }
    Ok(())
}

fn contains(value: &[u8], needle: &[u8]) -> bool {
    value.windows(needle.len()).any(|part| part == needle)
}

pub fn browser_id(key: Option<&BrowserKey>, headers: BrowserHeaders<'_>) -> Result<String, Error> {
    bounded(headers.user_agent, MAX_USER_AGENT_BYTES)?;
    let ua = headers.user_agent.unwrap_or_default();
    // PHP falsey strings include exactly "0". This branch precedes key use.
    if ua.is_empty() || ua == b"0" {
        return Ok("0deadbeef".into());
    }
    bounded(headers.mobile, 16)?;
    bounded(headers.model, MAX_MODEL_BYTES)?;
    bounded(headers.fetch_mode, 32)?;
    let key = key.ok_or(Error("Browser hint key is unavailable."))?;
    let mobile = [
        b"Android".as_slice(),
        b"iPhone",
        b"iPad",
        b"Dalvik",
        b"Clover",
        b"Kuroba",
        b"ChanOS",
    ]
    .iter()
    .any(|needle| contains(ua, needle))
        || headers.mobile == Some(b"?1");
    // Source /(\/|:)[.0-9]+/ is byte-oriented and removes only complete runs
    // following slash/colon. A bare delimiter or version-free suffix remains.
    let mut clean = Vec::with_capacity(ua.len() + MAX_MODEL_BYTES);
    let mut index = 0;
    while index < ua.len() {
        if matches!(ua[index], b'/' | b':')
            && ua
                .get(index + 1)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
        {
            index += 1;
            while ua
                .get(index)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
            {
                index += 1;
            }
        } else {
            clean.push(ua[index]);
            index += 1;
        }
    }
    if let Some(model) = headers
        .model
        .filter(|m| !m.is_empty() && *m != b"0" && *m != b"\"\"")
        && headers.fetch_mode != Some(b"navigate")
        && contains(ua, b"(Linux; Android 10; K)")
    {
        clean.extend_from_slice(model);
    }
    // Deliberate legacy comparison fingerprint, never a security MAC verifier.
    let signature = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, &key.0),
        &clean,
    );
    let mut result = if mobile { "1" } else { "0" }.to_owned();
    for byte in &signature.as_ref()[..4] {
        use std::fmt::Write;
        write!(result, "{byte:02x}").expect("String writes do not fail");
    }
    Ok(result)
}

/// Names must be in server-parser arrival order. HeaderMap iteration and a
/// client-supplied order field cannot provide this evidence. Duplicate names
/// retain their first case-insensitive position, matching one PHP server key.
pub fn request_signature(ordered_names: &[&str]) -> Result<String, Error> {
    if ordered_names.len() > MAX_HEADER_NAMES {
        return Err(Error("Ordered request headers are unavailable."));
    }
    let mut seen = HashSet::new();
    let mut picked = Vec::new();
    for name in ordered_names {
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return Err(Error("Ordered request headers are unavailable."));
        }
        let name = name.to_ascii_lowercase().replace('_', "-");
        if !seen.insert(name.clone()) {
            continue;
        }
        let code = match name.as_str() {
            "sec-ch-ua-platform" => 0,
            "sec-ch-ua-mobile" => 1,
            "sec-ch-ua-model" => 2,
            "user-agent" => 3,
            "accept-language" => 4,
            "sec-fetch-site" => 5,
            "sec-fetch-mode" => 6,
            "sec-fetch-dest" => 7,
            _ => continue,
        };
        picked.push(code);
    }
    let mut points = Vec::new();
    if picked.windows(3).any(|w| w == [5, 6, 7]) {
        points.push("fetch_smd");
    }
    if picked.windows(3).any(|w| w == [7, 6, 5]) {
        points.push("fetch_dms");
    }
    if picked.iter().any(|&n| n >= 5) {
        points.push("fetch_any");
    }
    if seen.contains("sec-gpc") || seen.contains("dnt") {
        points.push("dnt_gpc");
    }
    if seen.contains("x-requested-with") {
        points.push("xrw");
    }
    if picked.windows(3).any(|w| w.iter().all(|&n| n <= 2)) {
        points.push("ch_ua_block");
    }
    if ["user-agent", "accept", "referer", "accept-language"]
        .iter()
        .any(|name| !seen.contains(*name))
    {
        points.push("missing");
    }
    let input = if points.is_empty() {
        "deadbeef".into()
    } else {
        points.join("+")
    };
    let digest = Md5::digest(input.as_bytes());
    Ok(digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_match_isolated_source_with_synthetic_key_and_ordered_headers() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../fixtures/network-hints-reference.json"
        ))
        .unwrap();
        let key = BrowserKey::parse(fixture["synthetic_key_hex"].as_str().unwrap()).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1168);
        for (index, case) in cases.iter().enumerate() {
            let headers = case["headers"].as_array().unwrap();
            let value = |name: &str| {
                headers.iter().rev().find_map(|pair| {
                    (pair[0]
                        .as_str()
                        .unwrap()
                        .to_ascii_lowercase()
                        .replace('_', "-")
                        == name)
                        .then(|| pair[1].as_str().unwrap().as_bytes())
                })
            };
            let actual = browser_id(
                Some(&key),
                BrowserHeaders {
                    user_agent: value("user-agent"),
                    mobile: value("sec-ch-ua-mobile"),
                    model: value("sec-ch-ua-model"),
                    fetch_mode: value("sec-fetch-mode"),
                },
            )
            .unwrap();
            assert_eq!(
                actual,
                case["browser_id"].as_str().unwrap(),
                "browser case {index}"
            );
            let ordered: Vec<_> = headers
                .iter()
                .map(|pair| pair[0].as_str().unwrap())
                .collect();
            assert_eq!(
                request_signature(&ordered).unwrap(),
                case["request_signature"].as_str().unwrap(),
                "request case {index}"
            );
        }
    }

    #[test]
    fn malformed_or_unavailable_hints_cannot_be_silently_hashed_or_truncated() {
        for bad in [
            "".into(),
            "00".repeat(32),
            "gg".repeat(32),
            "11".repeat(31),
            "é".repeat(32),
        ] {
            assert!(BrowserKey::parse(&bad).is_err());
        }
        let key = BrowserKey::parse(&"11".repeat(32)).unwrap();
        assert_eq!(
            browser_id(None, BrowserHeaders::default()).unwrap(),
            "0deadbeef"
        );
        assert_eq!(
            browser_id(
                None,
                BrowserHeaders {
                    user_agent: Some(b"0"),
                    ..Default::default()
                }
            )
            .unwrap(),
            "0deadbeef"
        );
        assert!(
            browser_id(
                None,
                BrowserHeaders {
                    user_agent: Some(b"Desktop"),
                    ..Default::default()
                }
            )
            .is_err()
        );
        for ua in [
            vec![b'x'; MAX_USER_AGENT_BYTES + 1],
            b"Line\r\nInjection".to_vec(),
            b"Nul\0".to_vec(),
        ] {
            assert!(
                browser_id(
                    Some(&key),
                    BrowserHeaders {
                        user_agent: Some(&ua),
                        ..Default::default()
                    }
                )
                .is_err()
            );
        }
        let model = vec![b'x'; MAX_MODEL_BYTES + 1];
        assert!(
            browser_id(
                Some(&key),
                BrowserHeaders {
                    user_agent: Some(b"Desktop"),
                    model: Some(&model),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(request_signature(&vec!["accept"; MAX_HEADER_NAMES + 1]).is_err());
        for name in ["", "bad name", "bad:name", "bad\r\nname", "名前"] {
            assert!(request_signature(&[name]).is_err());
        }
    }

    #[test]
    fn request_order_and_php_normalized_duplicates_are_observable() {
        let required = ["user-agent", "accept", "referer", "accept-language"];
        let signature =
            |fetch: &[&str]| request_signature(&[required.as_slice(), fetch].concat()).unwrap();
        assert_eq!(
            signature(&["sec-fetch-site", "sec-fetch-mode", "sec-fetch-dest"]),
            "6dd64fdb"
        );
        assert_eq!(
            signature(&["sec-fetch-dest", "sec-fetch-mode", "sec-fetch-site"]),
            "a209695a"
        );
        assert_eq!(
            signature(&["sec-fetch-mode", "sec-fetch-site", "sec-fetch-dest"]),
            "3fc124ea"
        );
        assert_eq!(
            signature(&[
                "Sec-Fetch-Site",
                "sec_fetch_site",
                "sec-fetch-mode",
                "sec-fetch-dest"
            ]),
            "6dd64fdb"
        );
    }
}
