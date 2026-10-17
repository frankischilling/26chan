//! Bounded observations from the actual HTTP/1 parser. These are not a complete
//! PHP server snapshot, session authority, or a posting/filtering decision.
pub mod native_profile;

use crate::transport::RequestHeaderOrder;
use axum::http::HeaderMap;
use board_domain::content_admission::network::ProviderValue;

/// Deliberately has no Debug/serialization implementation: values may include
/// private cookies. HTTP fields remain separate from any SAPI server metadata.
pub struct CapturedRequest {
    http_fields: Vec<(String, Vec<u8>)>,
    cookies: ProviderValue<Vec<(String, Vec<u8>)>>,
}
impl CapturedRequest {
    /// CGI-shaped HTTP names in parser order. No non-HTTP entries are invented.
    pub fn http_fields(&self) -> &[(String, Vec<u8>)] {
        &self.http_fields
    }
    /// Qualified only for the explicitly supported flat-cookie subset. Raw
    /// HTTP_COOKIE remains in http_fields even if cookie parsing is unavailable.
    pub fn cookies(&self) -> ProviderValue<&[(String, Vec<u8>)]> {
        match &self.cookies {
            ProviderValue::Available(values) => ProviderValue::Available(values),
            ProviderValue::Unavailable => ProviderValue::Unavailable,
        }
    }
}

/// Missing parser metadata, duplicates, normalized-name collisions and changed
/// header membership are unavailable. Values are the current request values;
/// name-only metadata cannot detect a same-name value replacement by middleware.
/// Capture before any header-mutating middleware. Never substitute HeaderMap
/// iteration for order.
pub fn capture(
    headers: &HeaderMap,
    order: Option<&RequestHeaderOrder>,
) -> ProviderValue<CapturedRequest> {
    let Some(order) = order else {
        return ProviderValue::Unavailable;
    };
    if order.names().len() > 256 {
        return ProviderValue::Unavailable;
    }
    let names: Vec<_> = order.names().map(|name| name.as_str()).collect();
    match capture_ordered(headers, &names) {
        Some(value) => ProviderValue::Available(value),
        None => ProviderValue::Unavailable,
    }
}
fn capture_ordered(headers: &HeaderMap, names: &[&str]) -> Option<CapturedRequest> {
    if names.len() > 256 || names.len() != headers.len() {
        return None;
    }
    let mut fields: Vec<(String, Vec<u8>)> = Vec::with_capacity(names.len());
    let mut bytes = 0_usize;
    for name in names {
        let key = format!("HTTP_{}", name.replace('-', "_").to_ascii_uppercase());
        if key.len() > 128 || fields.iter().any(|(prior, _)| prior == &key) {
            return None;
        }
        let mut values = headers.get_all(*name).iter();
        let value = values.next()?.as_bytes();
        if values.next().is_some() || value.len() > 4096 {
            return None;
        }
        bytes = bytes.checked_add(key.len() + value.len())?;
        if bytes > 65536 {
            return None;
        }
        fields.push((key, value.to_vec()));
    }
    let raw_cookie = fields
        .iter()
        .find(|(key, _)| key == "HTTP_COOKIE")
        .map(|(_, value)| value.as_slice());
    let cookies = match flat_cookies(raw_cookie) {
        Some(values) => ProviderValue::Available(values),
        None => ProviderValue::Unavailable,
    };
    Some(CapturedRequest {
        http_fields: fields,
        cookies,
    })
}

// Flat ASCII cookie names and PHP raw-URL-decoded byte values. Name
// normalization, arrays, duplicate handling and quoted values remain unavailable.
fn flat_cookies(raw: Option<&[u8]>) -> Option<Vec<(String, Vec<u8>)>> {
    let Some(raw) = raw else {
        return Some(Vec::new());
    };
    if raw.len() > 4096 {
        return None;
    }
    if raw.is_empty() {
        return Some(Vec::new());
    }
    let mut result: Vec<(String, Vec<u8>)> = Vec::new();
    for part in raw.split(|b| *b == b';') {
        let part = part.strip_prefix(b" ").unwrap_or(part);
        let split = part.iter().position(|b| *b == b'=')?;
        let (name, rest) = part.split_at(split);
        let value = &rest[1..];
        if name.is_empty()
            || name.len() > 128
            || result.len() == 256
            || !(name[0].is_ascii_alphanumeric() || name[0] == b'_')
            || name.iter().all(u8::is_ascii_digit)
            || !name
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || !value
                .iter()
                .all(|b| matches!(b, 0x21..=0x7e) && !matches!(b, b'"' | b',' | b';' | b'\\'))
        {
            return None;
        }
        let name = std::str::from_utf8(name).ok()?;
        if result.iter().any(|(prior, _)| prior == name) {
            return None;
        }
        result.push((name.into(), decode_cookie_value(value)?));
    }
    Some(result)
}

fn decode_cookie_value(value: &[u8]) -> Option<Vec<u8>> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let mut decoded = Vec::with_capacity(value.len());
    let mut at = 0;
    while at < value.len() {
        if value[at] == b'%' {
            decoded.push(hex(*value.get(at + 1)?)? * 16 + hex(*value.get(at + 2)?)?);
            at += 3;
        } else {
            // PHP cookie parsing preserves literal plus; form decoding would
            // incorrectly replace it with a space. Decode percent escapes once.
            decoded.push(value[at]);
            at += 1;
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests;
