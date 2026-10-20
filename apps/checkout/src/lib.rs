#![forbid(unsafe_code)]
//! Authenticated sandbox notifications. No card or provider API credentials enter
//! this process. Live provider reconciliation is deliberately not implemented.
pub mod config;
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use board_domain::checkout::{
    Money,
    signature::{CallbackKey, MAX_BODY, Provider},
};
use board_store::checkout::{CheckoutStore, PaymentEvent};
use serde::Deserialize;
use std::sync::Arc;
#[derive(Clone)]
pub struct AppState {
    store: CheckoutStore,
    key: Arc<CallbackKey>,
}
impl AppState {
    pub fn new(store: CheckoutStore, key: CallbackKey) -> Self {
        Self {
            store,
            key: Arc::new(key),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    event: String,
    order: String,
    provider_object: String,
    product: String,
    amount_minor: i64,
    currency: String,
    kind: Kind,
    occurred_at: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Paid,
    Declined,
    Cancelled,
    Refunded,
    Disputed,
}
impl Kind {
    fn text(&self) -> &'static str {
        match self {
            Self::Paid => "paid",
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
            Self::Refunded => "refunded",
            Self::Disputed => "disputed",
        }
    }
}
fn response(status: StatusCode) -> Response {
    let mut response = status.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
async fn event(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if headers.get_all("x-checkout-signature").iter().count() != 1
        || headers.get_all(header::CONTENT_TYPE).iter().count() != 1
        || headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            != Some("application/json")
    {
        return response(StatusCode::BAD_REQUEST);
    }
    let Some(signature) = headers
        .get("x-checkout-signature")
        .and_then(|v| v.to_str().ok())
    else {
        return response(StatusCode::BAD_REQUEST);
    };
    let Some(verified) = state
        .key
        .verify(&body, signature, chrono::Utc::now().timestamp())
    else {
        return response(StatusCode::UNAUTHORIZED);
    };
    if verified.provider() != Provider::Sandbox {
        return response(StatusCode::SERVICE_UNAVAILABLE);
    }
    let Ok(input) = serde_json::from_slice::<Event>(verified.bytes()) else {
        return response(StatusCode::BAD_REQUEST);
    };
    if !uuid(&input.order)
        || !identifier(&input.event, 128)
        || !identifier(&input.provider_object, 255)
        || !matches!(input.product.as_str(), "pass_renewal" | "pass_purchase")
        || Money::new(input.amount_minor, &input.currency).is_none()
    {
        return response(StatusCode::BAD_REQUEST);
    }
    let Some(occurred) = chrono::DateTime::from_timestamp(input.occurred_at, 0) else {
        return response(StatusCode::BAD_REQUEST);
    };
    let fingerprint: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, verified.bytes())
        .as_ref()
        .try_into()
        .expect("SHA-256 size");
    let event = PaymentEvent {
        provider: "sandbox",
        event: &input.event,
        order: &input.order,
        object: &input.provider_object,
        product: &input.product,
        amount: input.amount_minor,
        currency: &input.currency,
        kind: input.kind.text(),
        occurred,
        fingerprint: &fingerprint,
    };
    match state.store.apply(&event).await {
        Ok(state) if matches!(state.as_str(), "applied" | "duplicate" | "review") => {
            response(StatusCode::NO_CONTENT)
        }
        Ok(state) if state == "capacity" => response(StatusCode::SERVICE_UNAVAILABLE),
        Ok(_) => response(StatusCode::BAD_REQUEST),
        Err(_) => response(StatusCode::SERVICE_UNAVAILABLE),
    }
}
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/events/sandbox", post(event))
        .fallback(|| async { response(StatusCode::NOT_FOUND) })
        .method_not_allowed_fallback(|| async { response(StatusCode::METHOD_NOT_ALLOWED) })
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}
