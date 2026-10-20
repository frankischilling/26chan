use crate::handlers::AppError;
use askama::Template;
use axum::{
    Json,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ACCEPT, VARY},
    },
    response::{IntoResponse, Redirect, Response},
};
use serde::Serialize;

#[derive(Clone, Copy)]
pub(crate) enum Format {
    Html,
    Json,
}

#[derive(Serialize)]
struct Posted {
    tid: i64,
    pid: i64,
}

#[derive(Serialize)]
struct Failed<'a> {
    error: &'a str,
}

// Exact S_RENZOKU_PASS suffix from config/global_strings.ini. Only this fixed
// fragment is emitted as markup; request or database text is never trusted HTML.
const PASS_COOLDOWN_HINT: &str = "<br>4chan Pass users have lower cooldowns. [<a href=\"https://www.4chan.org/pass\" target=\"_blank\">Learn More</a>]";

#[derive(Template)]
#[template(path = "posting_cooldown_error.html")]
struct CooldownMessage<'a> {
    message: &'a str,
}

#[derive(Template)]
#[template(path = "posting_duplicate_error.html")]
struct DuplicateMessage<'a> {
    location: &'a str,
}

impl Format {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        let mut accept = headers.get_all(ACCEPT).iter();
        if accept
            .next()
            .is_some_and(|value| value.as_bytes() == b"application/json")
            && accept.next().is_none()
        {
            Self::Json
        } else {
            Self::Html
        }
    }

    pub(crate) fn success(self, parent: i64, post: i64, location: &str) -> Response {
        match self {
            // The source returns zero for a newly created thread, not its ID.
            Self::Json => Json(Posted {
                tid: parent,
                pid: post,
            })
            .into_response(),
            Self::Html => Redirect::to(location).into_response(),
        }
    }

    pub(crate) fn error(self, error: AppError) -> Response {
        match self {
            Self::Html => error.into_response(),
            Self::Json => {
                // Source posting-rule failures use a successful HTTP envelope
                // containing only `error`. Infrastructure failures remain 5xx.
                let status = if error.0.is_client_error() {
                    StatusCode::OK
                } else {
                    error.0
                };
                (status, Json(Failed { error: error.1 })).into_response()
            }
        }
    }

    pub(crate) fn rule_error(self, message: &str) -> Response {
        match self {
            Self::Html => {
                crate::handlers::message_response(StatusCode::UNPROCESSABLE_ENTITY, message)
            }
            Self::Json => Json(Failed { error: message }).into_response(),
        }
    }

    pub(crate) fn image_duplicate_error(
        self,
        board: &str,
        rejection: &board_store::ImageDuplicateRefusal,
    ) -> Response {
        let Some((post, thread)) = rejection
            .existing
            .filter(|(post, thread)| *post > 0 && *thread > 0)
        else {
            return self.rule_error(board_domain::image_duplicates::DUPLICATE);
        };
        if board_domain::BoardSlug::parse(board).is_err() {
            return self.rule_error(board_domain::image_duplicates::DUPLICATE);
        }
        let location = format!("/{board}/thread/{thread}#p{post}");
        match self {
            Self::Html => match (DuplicateMessage {
                location: &location,
            })
            .render()
            {
                Ok(html) => {
                    (StatusCode::UNPROCESSABLE_ENTITY, axum::response::Html(html)).into_response()
                }
                Err(_) => self.rule_error(board_domain::image_duplicates::DUPLICATE),
            },
            Self::Json => Json(Failed {
                error: &format!("Error: Duplicate file exists. <a href=\"{location}\">here</a>."),
            })
            .into_response(),
        }
    }

    pub(crate) fn cooldown_error(
        self,
        rejection: &board_store::PostingCooldownRejection,
    ) -> Response {
        let message = rejection.source_message();
        if !rejection.pass_offer {
            return self.rule_error(&message);
        }
        match self {
            Self::Html => match (CooldownMessage { message: &message }).render() {
                Ok(html) => {
                    (StatusCode::UNPROCESSABLE_ENTITY, axum::response::Html(html)).into_response()
                }
                Err(_) => self.rule_error(&message),
            },
            Self::Json => Json(Failed {
                error: &format!("{message}{PASS_COOLDOWN_HINT}"),
            })
            .into_response(),
        }
    }

    pub(crate) fn invalid_form(self, error: crate::posting_form::Rejection) -> Response {
        match self {
            Self::Html => error.into_response(),
            // Parser and request-size failures retain their rejection status.
            Self::Json => (
                error.status(),
                Json(Failed {
                    error: error.message(),
                }),
            )
                .into_response(),
        }
    }

    pub(crate) fn finish(self, mut response: Response) -> Response {
        response
            .headers_mut()
            .append(VARY, HeaderValue::from_static("Accept"));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn only_one_exact_accept_value_selects_json() {
        for value in [
            "application/json",
            "application/json;q=1",
            "application/json, text/html",
            "*/*",
            "Application/JSON",
            "application/json ",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(ACCEPT, HeaderValue::from_str(value).unwrap());
            assert_eq!(
                matches!(Format::from_headers(&headers), Format::Json),
                value == "application/json"
            );
        }
        assert!(matches!(
            Format::from_headers(&HeaderMap::new()),
            Format::Html
        ));
        let mut repeated = HeaderMap::new();
        repeated.append(ACCEPT, HeaderValue::from_static("application/json"));
        repeated.append(ACCEPT, HeaderValue::from_static("text/html"));
        assert!(matches!(Format::from_headers(&repeated), Format::Html));
    }

    #[tokio::test]
    async fn duplicate_file_links_use_only_checked_local_targets() {
        let refusal = board_store::ImageDuplicateRefusal {
            existing: Some((i64::MAX, 9007199254740992)),
        };
        let expected = "/g/thread/9007199254740992#p9223372036854775807";
        for format in [Format::Html, Format::Json] {
            let response = format.image_duplicate_error("g", &refusal);
            assert_eq!(
                response.status(),
                if matches!(format, Format::Json) {
                    StatusCode::OK
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                }
            );
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body = String::from_utf8(bytes.to_vec()).unwrap();
            if matches!(format, Format::Json) {
                let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(
                    value["error"],
                    format!("Error: Duplicate file exists. <a href=\"{expected}\">here</a>.")
                );
            } else {
                assert!(body.contains(expected));
            }
        }
        for (board, existing) in [
            ("../outside", Some((1, 1))),
            ("g", Some((0, 1))),
            ("g", None),
        ] {
            let response = Format::Json
                .image_duplicate_error(board, &board_store::ImageDuplicateRefusal { existing });
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["error"], board_domain::image_duplicates::DUPLICATE);
        }
    }

    #[tokio::test]
    async fn exact_i64_identifiers_and_source_new_thread_zero_survive_encoding() {
        for parent in [0, i64::MAX - 1] {
            let response = Format::Json.success(parent, i64::MAX, "/unused");
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key("location"));
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                std::str::from_utf8(&bytes).unwrap(),
                format!("{{\"tid\":{parent},\"pid\":9223372036854775807}}")
            );
        }
    }

    #[tokio::test]
    async fn rule_failures_have_json_errors_while_dependency_failure_stays_unavailable() {
        for (status, expected) in [
            (StatusCode::UNPROCESSABLE_ENTITY, StatusCode::OK),
            (StatusCode::CONFLICT, StatusCode::OK),
            (StatusCode::NOT_FOUND, StatusCode::OK),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let response = Format::Json.error(AppError(status, "Denied <script> & \"quote\""));
            assert_eq!(response.status(), expected);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                value,
                serde_json::json!({"error": "Denied <script> & \"quote\""})
            );
        }
    }

    #[tokio::test]
    async fn actual_router_preserves_parser_origin_and_unavailable_statuses() {
        use axum::{body::Body, extract::ConnectInfo, http::Request};
        use rand_core::{OsRng, RngCore};
        use std::{net::SocketAddr, sync::Arc};
        use tower::ServiceExt;
        // Closed lazy pool performs no I/O. Malformed forms and origin checks
        // must reject before storage; valid forms report unavailable storage.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        pool.close().await;
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let key_text: String = key_bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let (app, api) = crate::routers_with_options(
            pool,
            crate::PublicRouterOptions {
                origin: "http://127.0.0.1:3000".into(),
                production: false,
                media: None,
                limits: board_config::PublicRequestLimits::default(),
                proxy_uid: None,
                poster_id_key: Some(Arc::new(
                    board_domain::poster_id::PosterIdKey::parse(&key_text).unwrap(),
                )),
                tripcode_key: None,
                country_database: None,
            },
        );
        let request = |body: String, content_type: &str, origin: &str| {
            Request::post("/test/post")
                // Model the listener's trusted transport metadata so valid
                // forms reach storage instead of the identity guard.
                .extension(ConnectInfo("192.0.2.1:1234".parse::<SocketAddr>().unwrap()))
                .header("accept", "application/json")
                .header("origin", origin)
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap()
        };
        let form_type = "application/x-www-form-urlencoded";
        let origin = "http://127.0.0.1:3000";
        for (body, content_type, status, message) in [
            (
                "resto=not-an-id&password=must-not-echo".to_owned(),
                form_type,
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid posting form.",
            ),
            (
                "{}".to_owned(),
                "application/json",
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Invalid posting form.",
            ),
            (
                format!("com={}", "x".repeat(262_145)),
                form_type,
                StatusCode::PAYLOAD_TOO_LARGE,
                "Invalid posting form.",
            ),
            (
                "com=Valid&password=owned-password".to_owned(),
                form_type,
                StatusCode::SERVICE_UNAVAILABLE,
                "Storage is unavailable. Try again later.",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(request(body, content_type, origin))
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()["content-type"], "application/json");
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(response.headers()["vary"], "Accept");
            assert!(!response.headers().contains_key("set-cookie"));
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                serde_json::json!({"error":message})
            );
        }
        assert_eq!(
            app.oneshot(request("".into(), form_type, "http://foreign.invalid"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            api.oneshot(request("".into(), form_type, origin))
                .await
                .unwrap()
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[tokio::test]
    async fn dynamic_rule_errors_escape_html_and_preserve_json_text() {
        use http_body_util::BodyExt;
        let message = "Owned <script>harmless</script> & \"quote\"";
        let html = Format::Html.rule_error(message);
        assert_eq!(html.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = html.into_body().collect().await.unwrap().to_bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("&#60;script&#62;harmless&#60;/script&#62;"));
        assert!(!text.contains("<script>harmless"));
        let json = Format::Json.rule_error(message);
        assert_eq!(json.status(), StatusCode::OK);
        let bytes = json.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"error":message})
        );
    }
    #[tokio::test]
    async fn cooldown_offer_has_fixed_html_and_source_json_without_changing_other_errors() {
        for offer in [false, true] {
            let rejection = board_store::PostingCooldownRejection {
                reason: board_store::PostingCooldownReason::Reply,
                remaining_seconds: 17,
                pass_offer: offer,
            };
            let response = Format::Html.cooldown_error(&rejection);
            assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let html = std::str::from_utf8(&bytes).unwrap();
            assert_eq!(html.contains("https://www.4chan.org/pass"), offer);
            if offer {
                assert!(html.contains("rel=\"noopener noreferrer\""));
            }
            let response = Format::Json.cooldown_error(&rejection);
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                serde_json::json!({"error":format!("{}{}",rejection.source_message(),if offer {PASS_COOLDOWN_HINT} else {""})})
            );
        }
        let html = CooldownMessage {
            message: "<script>owned</script>",
        }
        .render()
        .unwrap();
        assert!(html.contains("&#60;script&#62;owned&#60;/script&#62;"));
        assert!(!html.contains("<script>owned"));
    }
}
