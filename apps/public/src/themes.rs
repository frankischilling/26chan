//! Finite presentation preferences. No authentication or content authority.
use askama::Template;
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;

#[derive(Clone)]
struct Settings {
    origin: String,
    production: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Theme {
    #[default]
    Yotsuba,
    YotsubaB,
    Futaba,
    Burichan,
    Photon,
    Tomorrow,
}

impl Theme {
    const ALL: [Self; 6] = [
        Self::Yotsuba,
        Self::YotsubaB,
        Self::Futaba,
        Self::Burichan,
        Self::Tomorrow,
        Self::Photon,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Yotsuba => "yotsuba",
            Self::YotsubaB => "yotsuba-b",
            Self::Futaba => "futaba",
            Self::Burichan => "burichan",
            Self::Photon => "photon",
            Self::Tomorrow => "tomorrow",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Yotsuba => "Yotsuba",
            Self::YotsubaB => "Yotsuba B",
            Self::Futaba => "Futaba",
            Self::Burichan => "Burichan",
            Self::Photon => "Photon",
            Self::Tomorrow => "Tomorrow",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.id() == value)
    }

    fn css(self) -> &'static str {
        match self {
            Self::Yotsuba => "",
            Self::YotsubaB => include_str!("../static/themes/yotsuba-b.css"),
            Self::Futaba => include_str!("../static/themes/futaba.css"),
            Self::Burichan => include_str!("../static/themes/burichan.css"),
            Self::Photon => include_str!("../static/themes/photon.css"),
            Self::Tomorrow => include_str!("../static/themes/tomorrow.css"),
        }
    }
}

impl Settings {
    fn cookie_name(&self, worksafe: bool) -> &'static str {
        match (self.production, worksafe) {
            (true, true) => "__Host-board-theme-ws",
            (true, false) => "__Host-board-theme",
            (false, true) => "board-theme-ws",
            (false, false) => "board-theme",
        }
    }

    fn selection(&self, headers: &HeaderMap, default: Theme) -> Theme {
        let mut selected = None;
        let mut bytes = 0;
        for value in headers.get_all(header::COOKIE) {
            bytes += value.as_bytes().len();
            if bytes > 4096 {
                return default;
            }
            let Ok(value) = value.to_str() else {
                return default;
            };
            for pair in value.split(';') {
                if let Some((name, value)) = pair.trim().split_once('=')
                    && name == self.cookie_name(default == Theme::YotsubaB)
                {
                    // Duplicate and malformed preferences have one deterministic
                    // default, rather than first/last-cookie ambiguity.
                    if selected.is_some() {
                        return default;
                    }
                    let Some(theme) = Theme::parse(value) else {
                        return default;
                    };
                    selected = Some(theme);
                }
            }
        }
        selected.unwrap_or(default)
    }
}

/// Reused by the synthetic renderer; the normal app also applies its admission,
/// write-rate, timeout and security-header middleware around these routes.
pub fn routes<S: Clone + Send + Sync + 'static>(origin: String, production: bool) -> Router<S> {
    Router::new()
        .route("/settings/theme", get(page).post(update))
        .route("/static/theme.css", get(stylesheet))
        .route(
            "/static/themes/fade.png",
            get(|| async { background(include_bytes!("../static/themes/fade.png")) }),
        )
        .route(
            "/static/themes/fade-blue.png",
            get(|| async { background(include_bytes!("../static/themes/fade-blue.png")) }),
        )
        .layer(DefaultBodyLimit::max(1024))
        .with_state(Settings { origin, production })
}

#[derive(Template)]
#[template(path = "theme.html")]
struct ThemePage {
    options: Vec<(&'static str, &'static str, bool)>,
    return_to: String,
    worksafe: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultStyle {
    #[serde(default)]
    worksafe: bool,
}

impl DefaultStyle {
    fn theme(&self) -> Theme {
        if self.worksafe {
            Theme::YotsubaB
        } else {
            Theme::Yotsuba
        }
    }
}

async fn page(
    State(settings): State<Settings>,
    Query(default): Query<DefaultStyle>,
    headers: HeaderMap,
) -> Response {
    let return_to = headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= 2048)
        .and_then(|value| url::Url::parse(value).ok())
        .filter(|url| {
            url.origin().ascii_serialization() == settings.origin
                && url.username().is_empty()
                && url.password().is_none()
        })
        .map(|url| destination(url.path()))
        .unwrap_or_else(|| "/".into());
    let selected = settings.selection(&headers, default.theme());
    let page = ThemePage {
        options: Theme::ALL
            .into_iter()
            .map(|theme| (theme.id(), theme.label(), theme == selected))
            .collect(),
        return_to,
        worksafe: default.worksafe,
    };
    match page.render() {
        Ok(body) => private(Html(body).into_response()),
        Err(_) => private(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    theme: String,
    return_to: String,
    #[serde(default)]
    worksafe: bool,
}

async fn update(
    State(settings): State<Settings>,
    headers: HeaderMap,
    Form(form): Form<Selection>,
) -> Response {
    if headers.get_all(header::ORIGIN).iter().count() != 1
        || headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            != Some(settings.origin.as_str())
        || headers
            .get_all("sec-fetch-site")
            .iter()
            .any(|value| value != "same-origin" && value != "none")
    {
        return private(
            (StatusCode::FORBIDDEN, "A same-origin request is required.").into_response(),
        );
    }
    let Some(theme) = Theme::parse(&form.theme) else {
        return private((StatusCode::UNPROCESSABLE_ENTITY, "Unknown style.").into_response());
    };
    let mut response = private(Redirect::to(&destination(&form.return_to)).into_response());
    let secure = if settings.production { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Lax{}",
        settings.cookie_name(form.worksafe),
        theme.id(),
        secure
    );
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("fixed theme cookie"),
    );
    response
}

fn destination(path: &str) -> String {
    let parts: Vec<_> = path.split('/').collect();
    let slug = |value: &str| {
        !value.is_empty()
            && value.len() <= 10
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    };
    let number = |value: &str| {
        !value.is_empty()
            && value.len() <= 19
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<i64>().is_ok()
    };
    let allowed = path.len() <= 128
        && match parts.as_slice() {
            ["", ""] => true,
            ["", board, page] => {
                slug(board) && (matches!(*page, "" | "catalog" | "archive") || number(page))
            }
            ["", board, "thread" | "post", id] => slug(board) && number(id),
            _ => false,
        };
    if allowed { path.into() } else { "/".into() }
}

async fn stylesheet(
    State(settings): State<Settings>,
    Query(default): Query<DefaultStyle>,
    headers: HeaderMap,
) -> Response {
    let theme = settings.selection(&headers, default.theme());
    private(
        (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            format!(
                "{}\n{}",
                include_str!("../static/themes/common.css"),
                theme.css()
            ),
        )
            .into_response(),
    )
}

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
        .headers_mut()
        .append(header::VARY, HeaderValue::from_static("Cookie"));
    response
}

fn background(bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=0, must-revalidate"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn arbitrary_return_text_cannot_change_origin(value in ".{0,512}") {
            let path = destination(&value);
            let base = url::Url::parse("https://board.example/").unwrap();
            let resolved = base.join(&path).unwrap();
            prop_assert_eq!(resolved.origin(), base.origin());
            prop_assert!(resolved.query().is_none() && resolved.fragment().is_none());
            prop_assert!(path.len() <= 128);
            prop_assert!(path == "/" || path == value);
        }

        #[test]
        fn arbitrary_cookie_values_only_select_literal_styles(value in "[ -~]{0,256}") {
            let settings = Settings { origin: "https://board.example".into(), production: true };
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, format!("__Host-board-theme={value}").parse().unwrap());
            let selected = settings.selection(&headers, Theme::Yotsuba);
            prop_assert!(Theme::ALL.contains(&selected));
            let mut names = value.split(';');
            let first = names.next().unwrap().trim_end();
            if !value.contains(';') && Theme::parse(first).is_none() {
                prop_assert_eq!(selected, Theme::Yotsuba);
            }
        }
    }

    #[test]
    fn theme_cookie_is_finite_bounded_and_unambiguous() {
        let settings = Settings {
            origin: "http://127.0.0.1:3000".into(),
            production: false,
        };
        for theme in Theme::ALL {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                format!("unrelated=a; board-theme={}", theme.id())
                    .parse()
                    .unwrap(),
            );
            assert_eq!(settings.selection(&headers, Theme::Yotsuba), theme);
        }
        for cookie in [
            "board-theme=<script>",
            "board-theme=tomorrow; board-theme=photon",
            "board-theme=%74omorrow",
            "__Host-board-theme=tomorrow",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, cookie.parse().unwrap());
            assert_eq!(settings.selection(&headers, Theme::Yotsuba), Theme::Yotsuba);
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("unrelated={}; board-theme=tomorrow", "a".repeat(4096))
                .parse()
                .unwrap(),
        );
        assert_eq!(settings.selection(&headers, Theme::Yotsuba), Theme::Yotsuba);
        headers.clear();
        headers.append(header::COOKIE, "board-theme=tomorrow".parse().unwrap());
        headers.append(header::COOKIE, "board-theme=photon".parse().unwrap());
        assert_eq!(settings.selection(&headers, Theme::Yotsuba), Theme::Yotsuba);
    }

    #[test]
    fn return_navigation_only_selects_known_read_route_shapes() {
        for path in [
            "/",
            "/po/",
            "/po/catalog",
            "/po/archive",
            "/po/2",
            "/po/thread/123",
            "/po/post/123",
        ] {
            assert_eq!(destination(path), path);
        }
        for path in [
            "https://example.com/",
            "//example.com/",
            "/\\example.com/",
            "/%2fexample/",
            "/po/upload",
            "/po/upload/status",
            "/po/thread/1?token=secret",
            "/po/thread/-1",
            "/po/thread/99999999999999999999",
            "/po/post/1#p2",
            "/po/../admin",
        ] {
            assert_eq!(destination(path), "/");
        }
    }
}
