//! Snapshot-era event endpoints that are unconditionally closed in the source.
//! Do not create accounts, read event cookies, or consume request bodies here.
mod active;

use crate::AppState;
use axum::{
    Router,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};

pub(crate) fn routes(enabled: bool) -> Router<AppState> {
    if enabled {
        return active::routes();
    }
    Router::new()
        .route("/emotes_xa22.php", any(emotes_closed))
        .route("/xa24tb.php", any(market_closed))
        .route("/event-market.html", any(emotes_closed))
        .route("/event-emotes.html", any(emotes_closed))
}

async fn emotes_closed() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CACHE_CONTROL, "no-store")],
        "File not found.",
    )
        .into_response()
}

async fn market_closed() -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        r#"{"error":"The market is closed"}"#,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[tokio::test]
    async fn native_pages_are_absent_without_explicit_activation() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
            .unwrap();
        let web = crate::router(pool, "http://localhost:3000".into(), false);
        for path in ["/event-market.html", "/event-emotes.html"] {
            let response = web
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(!response.headers().contains_key(header::SET_COOKIE));
        }
    }
    #[tokio::test]
    async fn closed_source_actions_never_require_database_or_account_state() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/event-endpoint-reference.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let path = case["route"].as_str().unwrap();
            let status =
                StatusCode::from_u16(case["expected"]["status"].as_u64().unwrap() as u16).unwrap();
            let expected = case["expected"]["body"].as_str().unwrap();
            let mut fields = url::form_urlencoded::Serializer::new(String::new());
            for (name, value) in case["fields"].as_object().unwrap() {
                if let Some(array) = value.as_array() {
                    for value in array {
                        fields.append_pair(&format!("{name}[]"), value.as_str().unwrap());
                    }
                } else {
                    fields.append_pair(name, value.as_str().unwrap());
                }
            }
            let encoded = fields.finish();
            for method in [Method::GET, Method::HEAD, Method::POST] {
                // An unavailable pool proves that these closed responses do
                // not reach the dormant source's account/database workflow.
                let pool = PgPoolOptions::new()
                    .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
                    .unwrap();
                let web = crate::router(pool, "http://localhost:3000".into(), false);
                let response = web
                    .oneshot(
                        Request::builder()
                            .method(method.clone())
                            .uri(format!("{path}?{encoded}"))
                            .header("origin", "http://localhost:3000")
                            .header("cookie", "xa_sid=untrusted; 4chan_pass=untrusted")
                            .header("content-type", "application/x-www-form-urlencoded")
                            .body(Body::from(encoded.clone()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                assert!(!response.headers().contains_key(header::SET_COOKIE));
                assert!(!response.headers().contains_key(header::LOCATION));
                let body = to_bytes(response.into_body(), 1024).await.unwrap();
                assert_eq!(
                    body.as_ref(),
                    if method == Method::HEAD {
                        b""
                    } else {
                        expected.as_bytes()
                    }
                );
            }
        }
    }

    #[tokio::test]
    async fn closed_actions_keep_the_shared_cross_origin_write_refusal() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
            .unwrap();
        let web = crate::router(pool, "http://localhost:3000".into(), false);
        for path in ["/emotes_xa22.php", "/xa24tb.php"] {
            let response = web
                .clone()
                .oneshot(
                    Request::post(path)
                        .header("origin", "http://other.invalid")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }
}
