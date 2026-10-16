//! Browser-only, synthetic UI. This module never creates a posting proof.
use super::*;
use askama::Template;

#[derive(Template)]
#[template(path = "captcha_fixture.html")]
struct Fixture {
    captcha: page::View,
    quick_reply: bool,
}

pub(crate) fn response(mode: &str, origin: &str) -> Result<Response, String> {
    let parsed = url::Url::parse(origin).map_err(|_| "Invalid fixture origin.")?;
    if parsed.origin().ascii_serialization() != origin
        || !matches!(
            mode,
            "thread" | "reply" | "report" | "classic" | "noscript" | "qr"
        )
    {
        return Err("Invalid CAPTCHA fixture.".into());
    }
    let state = AppState {
        pool: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
            .map_err(|_| "Fixture pool unavailable.")?,
        origin: origin.into(),
        production: false,
        limits: std::sync::Arc::new(crate::security::Limits::new(
            board_config::PublicRequestLimits::default(),
        )),
        media: None,
        verification: None,
        proxy_uid: None,
        poster_id_key: None,
        tripcode_key: None,
        country_database: None,
    };
    let classic = matches!(mode, "classic" | "noscript");
    let captcha = page::View {
        enabled: true,
        classic,
        noscript_only: mode == "noscript",
        board: "g".into(),
        target: if mode == "thread" { 0 } else { 10 },
        purpose: match mode {
            "thread" => "thread",
            "report" => "report",
            _ => "reply",
        },
        csrf: if classic {
            "ab".repeat(32)
        } else {
            String::new()
        },
        token: if classic {
            format!("c1_{}", "cd".repeat(32))
        } else {
            String::new()
        },
        // A harmless 1x1 PNG makes native no-JavaScript form submission testable.
        image: if classic {
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a5XsAAAAASUVORK5CYII=".into()
        } else {
            String::new()
        },
        ttl_seconds: if classic { 120 } else { 0 },
        ..page::View::default()
    };
    let html = Fixture {
        captcha,
        quick_reply: mode == "qr",
    }
    .render()
    .map_err(|_| "Fixture rendering failed.")?;
    let mut response = axum::response::Html(html).into_response();
    response.extensions_mut().insert(page::CaptchaPage);
    Ok(if mode == "qr" {
        crate::security::posting_fixture_headers(response, &state)
    } else {
        crate::security::verification_fixture_headers(response, &state)
    })
}
