//! Qualification-only handler implementations. The production router deliberately
//! registers no v2 routes. Callers must apply the same authenticated protection
//! layer as v1 before routing these handlers in a qualification harness.
use crate::{
    AppState,
    http::{capability, error, single, store_error},
};
use axum::{
    Json,
    body::to_bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_store::media_intake::PairedInputDescriptor;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::{io, time::Duration};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationRequest {
    filename: String,
}

pub async fn reserve(State(state): State<AppState>, request: Request) -> Response {
    if single(request.headers(), "content-type") != Some("application/json") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let body = match to_bytes(request.into_body(), 1024).await {
        Ok(body) => body,
        Err(value) => {
            use std::error::Error;
            return error(
                if value
                    .source()
                    .is_some_and(|e| e.is::<http_body_util::LengthLimitError>())
                {
                    StatusCode::PAYLOAD_TOO_LARGE
                } else {
                    StatusCode::BAD_REQUEST
                },
            );
        }
    };
    let Ok(input) = serde_json::from_slice::<ReservationRequest>(&body) else {
        return error(StatusCode::BAD_REQUEST);
    };
    match state.store.reserve_pair(&input.filename).await {
        Ok(value) => (
            StatusCode::CREATED,
            Json(json!({"id":value.id,"capability":value.capability,"state":"receiving"})),
        )
            .into_response(),
        Err(value) => store_error(value),
    }
}

pub async fn upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let (object, capability) = match capability(request.headers(), &id) {
        Ok(value) => value,
        Err(status) => return error(status),
    };
    if single(request.headers(), "content-type") != Some("application/octet-stream") {
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let Ok(_permit) = state.uploads.try_acquire() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    if let Err(value) = state.store.begin_pair_upload(&id, &capability).await {
        return store_error(value);
    }
    let mut received_bytes = 0u64;
    let stream = request.into_body().into_data_stream().map(move |chunk| {
        let chunk = chunk.map_err(io::Error::other)?;
        received_bytes = received_bytes
            .checked_add(chunk.len() as u64)
            .filter(|n| *n <= board_media::paired::MAX_PAIR_INPUT_BYTES)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "paired input too large"))?;
        Ok::<_, io::Error>(chunk)
    });
    let reader = tokio_util::io::StreamReader::new(stream);
    let received = tokio::time::timeout(
        Duration::from_secs(15),
        state.quarantine.receive_pair(object, reader),
    )
    .await;
    let receipt = match received {
        Ok(Ok(receipt)) => receipt,
        failure => {
            let _ = state.store.abort_upload(&id, &capability).await;
            use board_media::{MediaError, paired::PairedError};
            return error(match failure {
                Err(_) => StatusCode::REQUEST_TIMEOUT,
                Ok(Err(MediaError::Paired(PairedError::InputSize) | MediaError::InputTooLarge)) => {
                    StatusCode::PAYLOAD_TOO_LARGE
                }
                Ok(Err(
                    MediaError::Paired(_) | MediaError::InputLengthMismatch | MediaError::Empty,
                )) => StatusCode::BAD_REQUEST,
                Ok(Err(MediaError::Io(ref e)))
                    if matches!(
                        e.kind(),
                        io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::Other
                            | io::ErrorKind::InvalidData
                    ) =>
                {
                    StatusCode::BAD_REQUEST
                }
                _ => StatusCode::SERVICE_UNAVAILABLE,
            });
        }
    };
    let descriptor = descriptor(&receipt);
    // A lost SQL response can hide a committed transition. Keep the immutable
    // completed object; never abort or unlink it because finalization is uncertain.
    match state
        .store
        .finish_pair_upload(&id, &capability, &descriptor)
        .await
    {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({"id":id,"state":"queued","input_bytes":receipt.bytes})),
        )
            .into_response(),
        Err(value) => store_error(value),
    }
}

fn descriptor(receipt: &board_media::PairReceipt) -> PairedInputDescriptor {
    let hex = |bytes: &[u8; 32]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    PairedInputDescriptor {
        bytes: receipt.bytes,
        sha256: hex(&receipt.sha256),
        image_bytes: receipt.image_bytes,
        image_sha256: hex(&receipt.image_sha256),
        replay_bytes: receipt.replay_bytes,
        replay_sha256: receipt.replay_sha256.as_ref().map(hex),
    }
}

/// Trusted, inactive reconciliation entry point after uncertain finalization.
/// It authenticates the same bearer, verifies the one completed object, and
/// retries only the exact SQL tuple. No new object or replacement is accepted.
pub async fn reconcile(
    state: &AppState,
    id: &str,
    capability: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _permit = state.uploads.try_acquire()?;
    let work = async {
        state.store.status(id, capability).await?;
        let receipt = state.quarantine.inspect_pair(id.parse()?).await?;
        state
            .store
            .finish_pair_upload(id, capability, &descriptor(&receipt))
            .await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    };
    tokio::time::timeout(Duration::from_secs(15), work).await?
}
