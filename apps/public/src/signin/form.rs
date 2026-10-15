//! Bounded sign-in inputs and a separate, HttpOnly browser form secret.
use axum::http::{HeaderMap, HeaderValue, header};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) const MAX_BODY: usize = 8192;
pub(super) struct FormSecret([u8; 32]);

fn name(production: bool) -> &'static str {
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
impl FormSecret {
    pub(super) fn generate() -> Result<Self, ()> {
        let mut secret = [0; 32];
        OsRng.try_fill_bytes(&mut secret).map_err(|_| ())?;
        Ok(Self(secret))
    }
    pub(super) fn read(headers: &HeaderMap, production: bool) -> Result<Option<Self>, ()> {
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
                if key != name(production) {
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
    pub(super) fn nonce(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"26chan:verification-form:v1\0");
        hash.update(self.0);
        hex(&hash.finalize())
    }
    pub(super) fn matches(&self, supplied: &str) -> bool {
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
    pub(super) fn append(&self, headers: &mut HeaderMap, production: bool) {
        set_cookie(headers, production, &hex(&self.0), 1800);
    }
    pub(super) fn clear(headers: &mut HeaderMap, production: bool) {
        set_cookie(headers, production, "", 0);
    }
}
fn set_cookie(headers: &mut HeaderMap, production: bool, value: &str, age: u16) {
    let secure = if production { "; Secure" } else { "" };
    let value = format!(
        "{}={value}; Path=/; Max-Age={age}; HttpOnly; SameSite=Strict{secure}",
        name(production)
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).expect("canonical verification cookie"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

pub(super) struct Fields(BTreeMap<String, String>);
impl Fields {
    pub(super) fn parse(input: &[u8], allowed: &[&str], limit: usize) -> Result<Self, ()> {
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
    pub(super) fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }
    pub(super) fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

pub(super) fn email_token(value: &str) -> bool {
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
