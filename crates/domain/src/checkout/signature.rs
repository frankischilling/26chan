//! Raw-body callback authentication. Stripe's v1 framing is documented in its
//! official stripe-node Webhooks implementation; sandbox signatures are isolated.
use ring::hmac;
pub const MAX_BODY: usize = 65_536;
pub const MAX_HEADER: usize = 1024;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Sandbox,
    Stripe,
}
pub struct CallbackKey {
    key: hmac::Key,
    provider: Provider,
}
pub struct VerifiedBody<'a> {
    bytes: &'a [u8],
    timestamp: i64,
    provider: Provider,
}
impl VerifiedBody<'_> {
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }
    pub fn provider(&self) -> Provider {
        self.provider
    }
}
impl CallbackKey {
    pub fn new(provider: Provider, secret: &str) -> Option<Self> {
        if !(32..=256).contains(&secret.len())
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return None;
        }
        if provider == Provider::Stripe && !secret.starts_with("whsec_") {
            return None;
        }
        Some(Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes()),
            provider,
        })
    }
    pub fn verify<'a>(&self, body: &'a [u8], header: &str, now: i64) -> Option<VerifiedBody<'a>> {
        if body.is_empty()
            || body.len() > MAX_BODY
            || header.is_empty()
            || header.len() > MAX_HEADER
            || !header.is_ascii()
            || now < 0
        {
            return None;
        }
        let mut timestamp = None;
        let mut signatures = Vec::new();
        let mut fields = 0;
        for field in header.split(',') {
            fields += 1;
            if fields > 12 {
                return None;
            }
            let (name, value) = field.split_once('=')?;
            match name {
                "t" => {
                    if timestamp.is_some()
                        || value.is_empty()
                        || value.len() > 12
                        || !value.bytes().all(|b| b.is_ascii_digit())
                        || (value.len() > 1 && value.starts_with('0'))
                    {
                        return None;
                    }
                    timestamp = Some(value.parse::<i64>().ok()?);
                }
                "v1" => {
                    if signatures.len() >= 4 {
                        return None;
                    }
                    signatures.push(decode(value)?);
                }
                "v0" => {
                    if value.len() > 128 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        let stamp = timestamp?;
        if stamp > now.saturating_add(30) || now.saturating_sub(stamp) > 300 {
            return None;
        }
        let mut content = Vec::with_capacity(body.len() + 64);
        if self.provider == Provider::Sandbox {
            content.extend_from_slice(b"26chan:checkout-sandbox:v1\0");
        }
        content.extend_from_slice(stamp.to_string().as_bytes());
        content.push(b'.');
        content.extend_from_slice(body);
        let mut valid = false;
        for signature in signatures {
            valid |= hmac::verify(&self.key, &content, &signature).is_ok();
        }
        valid.then_some(VerifiedBody {
            bytes: body,
            timestamp: stamp,
            provider: self.provider,
        })
    }
}
fn decode(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let nibble = |b| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut bytes = [0; 32];
    for (out, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *out = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Some(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn header(secret: &str, provider: Provider, stamp: i64, body: &[u8]) -> String {
        let mut content = if provider == Provider::Sandbox {
            b"26chan:checkout-sandbox:v1\0".to_vec()
        } else {
            Vec::new()
        };
        content.extend_from_slice(format!("{stamp}.").as_bytes());
        content.extend_from_slice(body);
        let signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes()),
            &content,
        );
        format!(
            "t={stamp},v1={}",
            signature
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }
    #[test]
    fn callbacks_require_exact_body_recent_timestamp_and_provider_key() {
        let secret = "whsec_owned_fixture_12345678901234567890";
        let body = br#"{"id":"owned-event"}"#;
        for provider in [Provider::Sandbox, Provider::Stripe] {
            let key = CallbackKey::new(provider, secret).unwrap();
            let signed = header(secret, provider, 1000, body);
            if provider == Provider::Stripe {
                // Independent Python stdlib HMAC vector for this exact raw body.
                assert_eq!(
                    signed,
                    "t=1000,v1=2d1dfa3737d5defdedcfe9b1a6cbfeb75b9e291913d9093c5f6b3f3f5f57ac59"
                );
            }
            let verified = key.verify(body, &signed, 1000).unwrap();
            assert_eq!(verified.bytes(), body);
            assert_eq!(verified.timestamp(), 1000);
            assert!(key.verify(body, &signed, 1300).is_some());
            assert!(key.verify(body, &signed, 1301).is_none());
            assert!(key.verify(body, &signed, 970).is_some());
            assert!(key.verify(body, &signed, 969).is_none());
            assert!(key.verify(b"{}", &signed, 1000).is_none());
            let other = if provider == Provider::Sandbox {
                Provider::Stripe
            } else {
                Provider::Sandbox
            };
            assert!(
                CallbackKey::new(other, secret)
                    .unwrap()
                    .verify(body, &signed, 1000)
                    .is_none()
            );
            for bad in [
                format!("{signed},t=1000"),
                signed.replace("t=1000", "t=01000"),
                signed.replace("v1=", "v0="),
                format!("{signed}\n"),
                format!("{signed},unknown=1"),
            ] {
                assert!(key.verify(body, &bad, 1000).is_none());
            }
            let rotated = format!(
                "t=1000,v1={},{}",
                "00".repeat(32),
                signed.split_once(',').unwrap().1
            );
            assert!(key.verify(body, &rotated, 1000).is_some());
        }
    }
}
