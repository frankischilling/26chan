//! One historical classifier namespace, separate from operator browser hints.
//! The compatibility material and derived identifier never leave this adapter.
use crate::content_admission::network::{
    ProviderValue,
    hints::{BrowserHeaders, Error, browser_id_material},
};

pub struct CompatibilityKey(Vec<u8>);
impl CompatibilityKey {
    /// Hex-encoded bytes, not a new random operator fingerprint key.
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.len() != 128 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error(
                "POSTING_BROWSER_COMPAT_KEY requires 128 hexadecimal digits.",
            ));
        }
        let mut bytes = Vec::with_capacity(value.len() / 2);
        for pair in value.as_bytes().chunks_exact(2) {
            let text = std::str::from_utf8(pair)
                .map_err(|_| Error("Invalid POSTING_BROWSER_COMPAT_KEY."))?;
            bytes.push(
                u8::from_str_radix(text, 16)
                    .map_err(|_| Error("Invalid POSTING_BROWSER_COMPAT_KEY."))?,
            );
        }
        Ok(Self(bytes))
    }
}
#[derive(Clone, Copy)]
pub struct Match(bool);
impl Match {
    pub(super) fn matched(self) -> bool {
        self.0
    }
    #[cfg(test)]
    pub(super) fn fixture(value: bool) -> Self {
        Self(value)
    }
}
pub fn classify(
    key: Option<&CompatibilityKey>,
    headers: Option<BrowserHeaders<'_>>,
) -> ProviderValue<Match> {
    classify_target(key, headers, "04d2237a2")
}
fn classify_target(
    key: Option<&CompatibilityKey>,
    headers: Option<BrowserHeaders<'_>>,
    target: &str,
) -> ProviderValue<Match> {
    let Some(headers) = headers else {
        return ProviderValue::Unavailable;
    };
    match browser_id_material(key.map(|key| key.0.as_slice()), headers) {
        Ok(id) => ProviderValue::Available(Match(id == target)),
        Err(_) => ProviderValue::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProviderValue::{Available as A, Unavailable as U};
    #[test]
    fn isolated_synthetic_source_vectors_preserve_the_compatibility_namespace() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../../fixtures/network-hints-reference.json"
        ))
        .unwrap();
        let key = CompatibilityKey(vec![0x11; 32]);
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1168);
        for case in cases {
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
            let input = || BrowserHeaders {
                user_agent: value("user-agent"),
                mobile: value("sec-ch-ua-mobile"),
                model: value("sec-ch-ua-model"),
                fetch_mode: value("sec-fetch-mode"),
            };
            let expected = case["browser_id"].as_str().unwrap();
            let A(matched) = classify_target(Some(&key), Some(input()), expected) else {
                panic!("complete synthetic case");
            };
            assert!(matched.matched());
            let A(other) = classify_target(Some(&key), Some(input()), "not-a-browser-id") else {
                panic!("complete synthetic case");
            };
            assert!(!other.matched());
            let A(actual) = classify(Some(&key), Some(input())) else {
                panic!("complete synthetic case");
            };
            assert_eq!(actual.matched(), expected == "04d2237a2");
        }
    }
    #[test]
    fn configured_material_uses_exactly_sixty_four_bytes() {
        let key = CompatibilityKey::parse(&"11".repeat(64)).unwrap();
        // Independent Python hashlib/HMAC vector with synthetic bytes only.
        let A(value) = classify_target(
            Some(&key),
            Some(BrowserHeaders {
                user_agent: Some(b"Firefox/123"),
                ..Default::default()
            }),
            "0e4ec2147",
        ) else {
            panic!("complete configured profile");
        };
        assert!(value.matched());
        assert_eq!(key.0, vec![0x11; 64]);
        let ascii = CompatibilityKey::parse(&"31".repeat(64)).unwrap();
        assert_eq!(ascii.0, vec![b'1'; 64]);
    }
    #[test]
    fn absent_key_is_unknown_except_the_source_falsey_user_agent_branch() {
        for ua in [None, Some(&b""[..]), Some(&b"0"[..])] {
            let A(value) = classify(
                None,
                Some(BrowserHeaders {
                    user_agent: ua,
                    ..Default::default()
                }),
            ) else {
                panic!("key-independent source branch");
            };
            assert!(!value.matched());
        }
        assert!(matches!(
            classify(
                None,
                Some(BrowserHeaders {
                    user_agent: Some(b"Firefox/123"),
                    ..Default::default()
                })
            ),
            U
        ));
        let key = CompatibilityKey::parse(&"11".repeat(64)).unwrap();
        assert!(matches!(classify(Some(&key), None), U));
        let oversized = vec![b'a'; 4097];
        assert!(matches!(
            classify(
                Some(&key),
                Some(BrowserHeaders {
                    user_agent: Some(&oversized),
                    ..Default::default()
                })
            ),
            U
        ));
        for invalid in [
            "".to_owned(),
            "1".repeat(31),
            "1".repeat(64),
            "z".repeat(128),
            "1".repeat(257),
        ] {
            assert!(CompatibilityKey::parse(&invalid).is_err());
        }
    }
}
