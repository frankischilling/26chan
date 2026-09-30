//! Optional display preferences; these cookies confer no posting authority.
use axum::http::{HeaderMap, HeaderValue, header};

fn present(headers: &HeaderMap, name: &str) -> bool {
    let mut bytes = 0;
    for value in headers.get_all(header::COOKIE) {
        bytes += value.as_bytes().len();
        if bytes > 4096 {
            return false;
        }
        let Ok(value) = value.to_str() else {
            return false;
        };
        if value.split(';').any(|pair| {
            pair.trim()
                .split_once('=')
                .is_some_and(|(key, _)| key == name)
        }) {
            return true;
        }
    }
    false
}

pub(crate) fn append(
    output: &mut HeaderMap,
    request: &HeaderMap,
    name: Option<&str>,
    options: &str,
    production: bool,
) {
    let name = name.map(|name| name.split('#').next().unwrap_or_default().trim());
    for (key, value) in [("4chan_name", name), ("options", Some(options))] {
        let Some(value) = value else { continue };
        if value.len() > board_domain::MAX_PUBLIC_FIELD_BYTES || value.chars().any(char::is_control)
        {
            continue;
        }
        if value.is_empty() && !present(request, key) {
            continue;
        }
        let value = url::form_urlencoded::byte_serialize(value.as_bytes())
            .collect::<String>()
            .replace('+', "%20");
        let age = if value.is_empty() { 0 } else { 31_536_000 };
        let secure = if production { "; Secure" } else { "" };
        let cookie = format!("{key}={value}; Path=/; Max-Age={age}; SameSite=Strict{secure}");
        output.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("bounded encoded preference"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(headers: &HeaderMap) -> Vec<&str> {
        headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect()
    }

    #[test]
    fn preferences_encode_display_text_and_discard_trip_secrets() {
        let mut headers = HeaderMap::new();
        append(
            &mut headers,
            &HeaderMap::new(),
            Some(" 名 + <user>##private-trip-secret "),
            "sage nonoko",
            true,
        );
        let cookies = values(&headers);
        assert_eq!(
            cookies,
            [
                "4chan_name=%E5%90%8D%20%2B%20%3Cuser%3E; Path=/; Max-Age=31536000; SameSite=Strict; Secure",
                "options=sage%20nonoko; Path=/; Max-Age=31536000; SameSite=Strict; Secure",
            ]
        );
        assert!(
            cookies
                .iter()
                .all(|value| !value.contains("private-trip-secret") && !value.contains("Domain="))
        );
    }

    #[test]
    fn empty_preferences_clear_existing_cookies_and_forced_names_are_untouched() {
        let mut output = HeaderMap::new();
        append(&mut output, &HeaderMap::new(), Some(""), "", false);
        assert!(output.is_empty());
        let mut request = HeaderMap::new();
        request.insert(
            header::COOKIE,
            "4chan_name=Owned; options=sage".parse().unwrap(),
        );
        append(&mut output, &request, Some("#secret"), "", false);
        assert_eq!(
            values(&output),
            [
                "4chan_name=; Path=/; Max-Age=0; SameSite=Strict",
                "options=; Path=/; Max-Age=0; SameSite=Strict",
            ]
        );
        output.clear();
        append(&mut output, &request, None, "sage", false);
        assert_eq!(
            values(&output),
            ["options=sage; Path=/; Max-Age=31536000; SameSite=Strict"]
        );
    }

    #[test]
    fn invalid_values_do_not_become_cookie_syntax() {
        for value in ["x\r\nSet-Cookie: injected", "\u{7f}", &"x".repeat(101)] {
            let mut output = HeaderMap::new();
            append(&mut output, &HeaderMap::new(), Some(value), value, true);
            assert!(output.is_empty());
        }
    }
}
