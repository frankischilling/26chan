use crate::handlers::AppError;
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
struct Failed {
    error: &'static str,
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
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;
        // Closed lazy pool performs no I/O. Malformed forms and origin checks
        // must reject before storage; valid forms report unavailable storage.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        pool.close().await;
        let (app, api) = crate::routers(pool, "http://127.0.0.1:3000".into(), false);
        let request = |body: String, content_type: &str, origin: &str| {
            Request::post("/test/post")
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
}
