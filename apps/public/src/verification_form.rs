//! Bounded verification inputs and purpose-separated HttpOnly form secrets.
use axum::http::{HeaderMap, HeaderValue, header};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(crate) const MAX_BODY: usize = 8192;
pub(crate) struct BrowserSecret<const PURPOSE: u8>([u8; 32]);
pub(crate) type FormSecret = BrowserSecret<0>;
pub(crate) type PostingSecret = BrowserSecret<1>;
pub(crate) type PassSecret = BrowserSecret<2>;
pub(crate) type RecoverySecret = BrowserSecret<3>;

fn name<const PURPOSE: u8>(production: bool) -> &'static str {
    if PURPOSE == 3 {
        return if production {
            "__Host-board-pass-recovery"
        } else {
            "board-pass-recovery"
        };
    }
    if PURPOSE == 2 {
        return if production {
            "__Host-board-pass-form"
        } else {
            "board-pass-form"
        };
    }
    if PURPOSE == 1 {
        return if production {
            "__Host-board-captcha"
        } else {
            "board-captcha"
        };
    }
    if production {
        "__Host-board-verification"
    } else {
        "board-verification"
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(DIGITS[(b >> 4) as usize]),
                char::from(DIGITS[(b & 15) as usize]),
            ]
        })
        .collect()
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
    let mut output = [0; 32];
    for (out, pair) in output.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *out = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Some(output)
}
impl<const PURPOSE: u8> BrowserSecret<PURPOSE> {
    pub(crate) fn generate() -> Result<Self, ()> {
        let mut secret = [0; 32];
        OsRng.try_fill_bytes(&mut secret).map_err(|_| ())?;
        Ok(Self(secret))
    }
    pub(crate) fn read(headers: &HeaderMap, production: bool) -> Result<Option<Self>, ()> {
        let mut total = 0usize;
        let mut found = None;
        let mut seen = false;
        for value in headers.get_all(header::COOKIE) {
            total = total.checked_add(value.as_bytes().len()).ok_or(())?;
            if total > MAX_BODY {
                return Err(());
            }
            for pair in value.to_str().map_err(|_| ())?.split(';') {
                let Some((key, value)) = pair.trim().split_once('=') else {
                    continue;
                };
                if key != name::<PURPOSE>(production) {
                    continue;
                }
                if seen {
                    return Err(());
                }
                seen = true;
                found = decode(value).map(Self);
            }
        }
        Ok(found)
    }
    pub(crate) fn nonce(&self) -> String {
        let mut hash = Sha256::new();
        if PURPOSE == 1 {
            hash.update(b"26chan:posting-form:v1\0");
        } else if PURPOSE == 3 {
            hash.update(b"26chan:pass-recovery-form:v1\0");
        } else if PURPOSE == 2 {
            hash.update(b"26chan:pass-form:v1\0");
        } else {
            hash.update(b"26chan:verification-form:v1\0");
        }
        hash.update(self.0);
        hex(&hash.finalize())
    }
    pub(crate) fn matches(&self, supplied: &str) -> bool {
        // This is a public form nonce, not the cookie secret. A fixed-length
        // comparison also avoids accepting alternate encodings.
        let Some(supplied) = decode(supplied) else {
            return false;
        };
        let expected = decode(&self.nonce()).expect("canonical form nonce");
        supplied
            .iter()
            .zip(expected)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
    pub(crate) fn append(&self, headers: &mut HeaderMap, production: bool) {
        set_cookie::<PURPOSE>(headers, production, &hex(&self.0), 1800);
    }
    pub(crate) fn clear(headers: &mut HeaderMap, production: bool) {
        set_cookie::<PURPOSE>(headers, production, "", 0);
    }
}
fn set_cookie<const PURPOSE: u8>(headers: &mut HeaderMap, production: bool, value: &str, age: u16) {
    let secure = if production { "; Secure" } else { "" };
    let value = format!(
        "{}={value}; Path=/; Max-Age={age}; HttpOnly; SameSite=Strict{secure}",
        name::<PURPOSE>(production)
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).expect("canonical verification cookie"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

pub(crate) struct Fields(BTreeMap<String, String>);
impl Fields {
    pub(crate) fn parse(input: &[u8], allowed: &[&str], limit: usize) -> Result<Self, ()> {
        if input.len() > limit || input.iter().filter(|&&b| b == b'&').count() >= allowed.len() {
            return Err(());
        }
        // form_urlencoded deliberately tolerates malformed escapes and UTF-8.
        // These security-sensitive forms require an unambiguous encoding.
        let mut pos = 0;
        while pos < input.len() {
            if input[pos] == b'%' {
                if pos + 2 >= input.len()
                    || !input[pos + 1..pos + 3].iter().all(u8::is_ascii_hexdigit)
                {
                    return Err(());
                }
                pos += 3;
            } else {
                pos += 1;
            }
        }
        let mut fields = BTreeMap::new();
        for (key, value) in url::form_urlencoded::parse(input) {
            if !allowed.contains(&key.as_ref())
                || value.contains('\u{fffd}')
                || value.contains('\0')
                || fields
                    .insert(key.into_owned(), value.into_owned())
                    .is_some()
            {
                return Err(());
            }
        }
        Ok(Self(fields))
    }
    pub(crate) fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }
    pub(crate) fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

pub(crate) fn email_token(value: &str) -> bool {
    value.strip_prefix("e1_").and_then(decode).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cookies_are_separate_private_and_unambiguous() {
        let secret = FormSecret::generate().unwrap();
        let mut output = HeaderMap::new();
        secret.append(&mut output, true);
        let value = output[header::SET_COOKIE].to_str().unwrap();
        assert!(value.starts_with("__Host-board-verification="));
        assert!(value.ends_with("Path=/; Max-Age=1800; HttpOnly; SameSite=Strict; Secure"));
        assert!(!value.contains(&secret.nonce()));
        let mut input = HeaderMap::new();
        input.append(
            header::COOKIE,
            value.split(';').next().unwrap().parse().unwrap(),
        );
        assert!(
            FormSecret::read(&input, true)
                .unwrap()
                .unwrap()
                .matches(&secret.nonce())
        );
        assert!(FormSecret::read(&input, false).unwrap().is_none());
        input.append(
            header::COOKIE,
            value.split(';').next().unwrap().parse().unwrap(),
        );
        assert!(FormSecret::read(&input, true).is_err());
        assert!(!secret.matches(&hex(&secret.0)));
        assert!(!secret.matches(&secret.nonce().to_uppercase()));
        assert!(!secret.matches(&FormSecret::generate().unwrap().nonce()));
    }
    #[test]
    fn posting_and_signin_secrets_never_share_cookies_or_nonces() {
        let signin = BrowserSecret::<0>([7; 32]);
        let posting = BrowserSecret::<1>([7; 32]);
        let pass = BrowserSecret::<2>([7; 32]);
        let recovery = BrowserSecret::<3>([7; 32]);
        for other in [signin.nonce(), posting.nonce(), pass.nonce()] {
            assert_ne!(recovery.nonce(), other);
            assert!(!recovery.matches(&other));
        }
        for production in [false, true] {
            let mut output = HeaderMap::new();
            recovery.append(&mut output, production);
            let value = output[header::SET_COOKIE].to_str().unwrap();
            assert!(value.starts_with(if production {
                "__Host-board-pass-recovery="
            } else {
                "board-pass-recovery="
            }));
            let mut input = HeaderMap::new();
            input.insert(
                header::COOKIE,
                value.split(';').next().unwrap().parse().unwrap(),
            );
            assert!(
                RecoverySecret::read(&input, production)
                    .unwrap()
                    .unwrap()
                    .matches(&recovery.nonce())
            );
            assert!(PassSecret::read(&input, production).unwrap().is_none());
            assert!(FormSecret::read(&input, production).unwrap().is_none());
        }
        assert_ne!(pass.nonce(), signin.nonce());
        assert_ne!(pass.nonce(), posting.nonce());
        assert!(!pass.matches(&signin.nonce()));
        assert_ne!(signin.nonce(), posting.nonce());
        assert!(!posting.matches(&signin.nonce()));
        assert!(!signin.matches(&posting.nonce()));
        for production in [false, true] {
            let mut output = HeaderMap::new();
            posting.append(&mut output, production);
            let value = output[header::SET_COOKIE].to_str().unwrap();
            assert!(value.starts_with(if production {
                "__Host-board-captcha="
            } else {
                "board-captcha="
            }));
            assert!(value.contains("HttpOnly; SameSite=Strict"));
            let mut input = HeaderMap::new();
            input.insert(
                header::COOKIE,
                value.split(';').next().unwrap().parse().unwrap(),
            );
            assert!(
                PostingSecret::read(&input, production)
                    .unwrap()
                    .unwrap()
                    .matches(&posting.nonce())
            );
            assert!(FormSecret::read(&input, production).unwrap().is_none());
        }
    }

    #[test]
    fn form_limits_and_duplicate_decoded_names_fail_closed() {
        let allowed = ["action", "csrf", "email"];
        for input in [
            "action=x&action=y",
            "action=x&%61ction=y",
            "email=%FF",
            "email=%",
            "email=%0Z",
            "email=%00",
            "unknown=x",
            "action=x&&&&",
        ] {
            assert!(
                Fields::parse(input.as_bytes(), &allowed, MAX_BODY).is_err(),
                "{input}"
            );
        }
        let fields = Fields::parse(
            b"action=request&email=a%2Bb%40example.test",
            &allowed,
            MAX_BODY,
        )
        .unwrap();
        assert_eq!(fields.get("email"), "a+b@example.test");
        assert!(!fields.has("csrf"));
        assert!(Fields::parse(&vec![b'x'; MAX_BODY + 1], &allowed, MAX_BODY).is_err());
        assert!(email_token(&format!("e1_{}", "ab".repeat(32))));
        assert!(!email_token(&format!("e1_{}", "AB".repeat(32))));
    }
}
