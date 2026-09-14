//! Optional, short-lived public post hints. These cookies grant no authority.
use axum::http::{HeaderMap, HeaderValue, header};

pub(crate) struct Receipt<'a> {
    pub board: &'a str,
    pub thread: i64,
    pub post: i64,
    pub track: bool,
    pub watch: bool,
    pub production: bool,
}

impl Receipt<'_> {
    pub(crate) fn append(self, response: &mut HeaderMap, request: &HeaderMap) {
        let Self {
            board,
            thread,
            post,
            track,
            watch,
            production,
        } = self;
        let secure = if production { "; Secure" } else { "" };
        let scope = format!("Path=/{board}/; SameSite=Strict{secure}");
        let mut cookie = |value: String| {
            response.append(
                header::SET_COOKIE,
                HeaderValue::from_str(&value).expect("canonical public post receipt"),
            );
        };
        if watch && thread == post {
            cookie(format!("4chan_awt={thread}; Max-Age=120; {scope}"));
        }
        if !track {
            return;
        }
        let raw = request
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        // Do not let an oversized cookie header create unbounded receipt work.
        if raw.len() > 8192 {
            return;
        }
        let mut existing: Vec<_> = raw
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .filter_map(|(name, _)| name.strip_prefix("board-posted-"))
            .filter_map(|id| {
                id.parse::<i64>()
                    .ok()
                    .filter(|parsed| *parsed > 0 && parsed.to_string() == id)
            })
            .take(64)
            .collect();
        existing.sort_unstable();
        existing.dedup();
        for id in existing.iter().take(existing.len().saturating_sub(31)) {
            cookie(format!("board-posted-{id}=; Max-Age=0; {scope}"));
        }
        cookie(format!(
            "board-posted-{post}={thread}.{}; Max-Age=120; {scope}",
            u8::from(watch && thread == post)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_names_are_bounded_and_oldest_owned_names_expire() {
        let mut request = HeaderMap::new();
        request.insert(
            header::COOKIE,
            HeaderValue::from_str(
                &(1..=40)
                    .map(|id| format!("board-posted-{id}=1.0"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
            .unwrap(),
        );
        let mut response = HeaderMap::new();
        Receipt {
            board: "demo",
            thread: 50,
            post: 51,
            track: true,
            watch: true,
            production: true,
        }
        .append(&mut response, &request);
        let cookies: Vec<_> = response
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(cookies.len(), 10);
        assert!(cookies[0].starts_with("board-posted-1=; Max-Age=0;"));
        assert!(cookies[8].starts_with("board-posted-9=; Max-Age=0;"));
        assert!(cookies[9].starts_with("board-posted-51=50.0; Max-Age=120;"));
        assert!(
            cookies
                .iter()
                .all(|cookie| cookie.ends_with("Path=/demo/; SameSite=Strict; Secure"))
        );
        assert!(
            cookies
                .iter()
                .all(|cookie| !cookie.contains("Domain=") && !cookie.contains("HttpOnly"))
        );
    }
}
