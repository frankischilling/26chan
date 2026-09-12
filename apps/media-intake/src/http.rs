use crate::{Access, AppState, config::valid_token};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use board_media::{MediaError, ObjectId};
use board_store::StoreError;
use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::json;
use std::{io, time::Duration};

pub(crate) fn error(status: StatusCode) -> Response {
    let mut response = (
        status,
        Json(json!({"error":status.canonical_reason().unwrap_or("Request rejected")})),
    )
        .into_response();
    if status == StatusCode::SERVICE_UNAVAILABLE {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

fn single<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    Some(value)
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = single(headers, "authorization").and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    value.len() == token.len()
        && value
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

pub(crate) async fn protect(
    State(access): State<Access>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = if !authorized(request.headers(), &access.token) {
        error(StatusCode::UNAUTHORIZED)
    } else if request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site")
    {
        error(StatusCode::FORBIDDEN)
    } else if let Ok(permit) = access.requests.try_acquire_owned() {
        let response = tokio::time::timeout(Duration::from_secs(20), next.run(request))
            .await
            .unwrap_or_else(|_| error(StatusCode::REQUEST_TIMEOUT));
        let response = if response.status().is_client_error() || response.status().is_server_error()
        {
            error(response.status())
        } else {
            response
        };
        board_http::hold_permit(response, permit)
    } else {
        error(StatusCode::SERVICE_UNAVAILABLE)
    };
    for (name, value) in [
        ("cache-control", "private, no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("referrer-policy", "no-referrer"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationRequest {
    filename: String,
}

pub(crate) async fn reserve(State(state): State<AppState>, request: Request) -> Response {
    if single(request.headers(), "content-type") != Some("application/json") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let body = match to_bytes(request.into_body(), 1024).await {
        Ok(body) => body,
        Err(error_value) => {
            use std::error::Error;
            let oversized = error_value
                .source()
                .is_some_and(|e| e.is::<http_body_util::LengthLimitError>());
            return error(if oversized {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            });
        }
    };
    let Ok(input) = serde_json::from_slice::<ReservationRequest>(&body) else {
        return error(StatusCode::BAD_REQUEST);
    };
    match state.store.reserve(&input.filename).await {
        Ok(reservation) => (StatusCode::CREATED, Json(json!({"id":reservation.id,"capability":reservation.capability,"state":"receiving"}))).into_response(),
        Err(StoreError::Conflict(_)) => error(StatusCode::SERVICE_UNAVAILABLE),
        Err(value) => store_error(value),
    }
}

fn capability(headers: &HeaderMap, id: &str) -> Result<(ObjectId, String), StatusCode> {
    let id = id.parse().map_err(|_| StatusCode::NOT_FOUND)?;
    let value = single(headers, "upload-capability")
        .filter(|v| valid_token(v))
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok((id, value.to_owned()))
}

pub(crate) async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let (object, capability) = match capability(request.headers(), &id) {
        Ok(v) => v,
        Err(e) => return error(e),
    };
    if single(request.headers(), "content-type") != Some("application/octet-stream") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let Ok(_permit) = state.uploads.try_acquire() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    if let Err(value) = state.store.begin_upload(&id, &capability).await {
        return store_error(value);
    }
    let stream = request
        .into_body()
        .into_data_stream()
        .map_err(io::Error::other);
    let reader = tokio_util::io::StreamReader::new(stream);
    let received = tokio::time::timeout(
        Duration::from_secs(15),
        state.quarantine.receive(object, reader),
    )
    .await;
    let bytes = match received {
        Ok(Ok(bytes)) => bytes,
        failure => {
            // A dropped receive future removes its own partial file. If SQL is
            // unavailable, the claimed receiving row expires for reconciliation.
            let _ = state.store.abort_upload(&id, &capability).await;
            return error(match failure {
                Err(_) => StatusCode::REQUEST_TIMEOUT,
                Ok(Err(MediaError::InputTooLarge)) => StatusCode::PAYLOAD_TOO_LARGE,
                Ok(Err(MediaError::Empty)) => StatusCode::BAD_REQUEST,
                Ok(Err(MediaError::Io(ref e))) if e.kind() == io::ErrorKind::Other => {
                    StatusCode::BAD_REQUEST
                }
                _ => StatusCode::SERVICE_UNAVAILABLE,
            });
        }
    };
    // Keep complete input after any uncertain database result. A commit may
    // have succeeded even if its response was lost; cleanup must use queue state.
    match state.store.finish_upload(&id, &capability, bytes).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"id":id,"state":"queued","input_bytes":bytes})),
        )
            .into_response(),
        Err(value) => store_error(value),
    }
}

pub(crate) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let (_, capability) = match capability(request.headers(), &id) {
        Ok(v) => v,
        Err(e) => return error(e),
    };
    match state.store.status(&id, &capability).await {
        Ok(value) => {
            let mut body = json!({"id":value.id,"state":value.state});
            if let Some(bytes) = value.input_bytes {
                body["input_bytes"] = json!(bytes);
            }
            if let Some(id) = value.output_id {
                body["output_id"] = json!(id);
            }
            Json(body).into_response()
        }
        Err(value) => store_error(value),
    }
}

pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    if state.store.ready().await.is_err() || state.quarantine.ready().is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    Json(json!({"status":"ok"})).into_response()
}

fn store_error(value: StoreError) -> Response {
    error(match value {
        StoreError::NotFound => StatusCode::NOT_FOUND,
        StoreError::Conflict(_) => StatusCode::CONFLICT,
        StoreError::Invalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    })
}
