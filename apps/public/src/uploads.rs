use crate::{
    AppState,
    handlers::AppError,
    intake,
    posting_response::Format,
    views::{UploadForm, UploadPage},
};
use axum::{
    Form,
    body::Body,
    extract::{FromRequest, Multipart, Path, Request, State, rejection::FormRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use bytes::Bytes;
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum UploadState {
    Queued,
    Processing,
    Approved,
    Failed,
    Incomplete,
}

#[derive(Serialize)]
struct Receipt<'a> {
    upload_id: &'a str,
    upload_capability: &'a str,
    // Identifiers never cross the browser's floating-point number boundary.
    resto: String,
    state: UploadState,
}

fn json(state: &AppState, value: &impl Serialize) -> Result<Response, AppError> {
    let bytes = crate::output::json(state.limits.response_writer(1024), value)?;
    Ok(([("content-type", "application/json")], bytes.into_body()).into_response())
}

fn finish(state: &AppState, format: Format, result: Result<Response, AppError>) -> Response {
    let mut response = match result {
        Ok(response) => response,
        Err(error) => match format {
            Format::Html => error.into_response(),
            Format::Json => {
                #[derive(Serialize)]
                struct Failure {
                    error: &'static str,
                }
                match json(state, &Failure { error: error.1 }) {
                    Ok(mut response) => {
                        *response.status_mut() = error.0;
                        response
                    }
                    Err(error) => error.into_response(),
                }
            }
        },
    };
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    format.finish(response)
}

fn parsed(form: Result<Form<UploadForm>, FormRejection>) -> Result<UploadForm, AppError> {
    // Framework rejection text can include input. Native failures use only a
    // fixed message; the parser's actual HTTP status is retained.
    form.map(|Form(form)| form)
        .map_err(|error| AppError(error.status(), "Invalid upload request."))
}

fn invalid() -> AppError {
    AppError(
        StatusCode::UNPROCESSABLE_ENTITY,
        "Choose one file of at most 8 MiB.",
    )
}

async fn settings(
    state: &AppState,
    board: &str,
    resto: i64,
) -> Result<board_store::Board, AppError> {
    if state.media.is_none() {
        return Err(AppError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Media uploads are unavailable.",
        ));
    }
    let settings = board_store::board(&state.pool, board).await?;
    if settings.image_limit == 0 {
        return Err(AppError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "This board does not accept images.",
        ));
    }
    if resto < 0 {
        return Err(invalid());
    }
    settings.check_attachment_allowed(resto, true)?;
    if resto > 0 {
        let thread = board_store::thread(&state.pool, board, resto).await?;
        if thread.archived_at.is_some()
            || thread.closed
            || (thread.reply_count >= settings.reply_limit
                && !(thread.sticky && thread.undead && settings.reply_limit > 1))
        {
            return Err(AppError(
                StatusCode::CONFLICT,
                "This thread is closed or full.",
            ));
        }
    }
    Ok(settings)
}

fn page(
    state: &AppState,
    board: board_store::Board,
    form: UploadForm,
    stage: UploadState,
    message: &'static str,
    format: Format,
) -> Result<Response, AppError> {
    if matches!(format, Format::Json) {
        return json(
            state,
            &Receipt {
                upload_id: &form.upload_id,
                upload_capability: &form.upload_capability,
                resto: form.resto.to_string(),
                state: stage,
            },
        );
    }
    Ok((
        [("cache-control", "private, no-store")],
        crate::output::html(
            state,
            &UploadPage {
                public_origin: state.origin.clone(),
                board,
                form,
                ready: matches!(stage, UploadState::Approved),
                message,
            },
        )?,
    )
        .into_response())
}

pub async fn upload(
    State(state): State<AppState>,
    Path(board): Path<String>,
    request: Request,
) -> Response {
    let format = Format::from_headers(request.headers());
    let result = receive(&state, &board, request, format).await;
    finish(&state, format, result)
}

async fn receive(
    state: &AppState,
    board: &str,
    request: Request,
    format: Format,
) -> Result<Response, AppError> {
    let media = state.media.as_ref().ok_or(AppError(
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Media uploads are unavailable.",
    ))?;
    let _permit = state.limits.uploads.try_acquire().map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Uploads are busy. Try again later.",
        )
    })?;
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|_| invalid())?;
    let mut parent = multipart
        .next_field()
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if parent.name() != Some("resto") || parent.file_name().is_some() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = parent.chunk().await.map_err(|_| invalid())? {
        if bytes.len() + chunk.len() > 20 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    let resto = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(invalid)?;
    drop(parent);
    let board_settings = settings(state, board, resto).await?;
    let mut field = multipart
        .next_field()
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if field.name() != Some("upfile") {
        return Err(invalid());
    }
    let filename = field
        .file_name()
        .filter(|name| !name.is_empty() && name.len() <= 255 && !name.chars().any(char::is_control))
        .ok_or_else(invalid)?
        .to_owned();
    let reservation = media.reserve(&filename).await?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let send = media.upload(
        &reservation.id,
        &reservation.capability,
        Body::from_stream(stream),
    );
    let pump = async {
        let mut total = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|_| invalid())? {
            total = total.checked_add(chunk.len()).ok_or_else(invalid)?;
            if total > 8_388_608 {
                return Err(AppError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "The file exceeds the 8 MiB upload limit.",
                ));
            }
            // Copy only bounded chunks into the one-slot channel; a Bytes slice
            // would retain a potentially much larger inbound allocation.
            for piece in chunk.chunks(16_384) {
                sender
                    .send(Ok(Bytes::copy_from_slice(piece)))
                    .await
                    .map_err(|_| intake::unavailable())?;
            }
        }
        drop(field);
        if total == 0 {
            return Err(invalid());
        }
        drop(sender);
        Ok(())
    };
    if let Err(error) = tokio::try_join!(send, pump) {
        // Revoke attachment authority even if the remote completion was uncertain.
        // Cleanup remains responsible for bounded input and unused output files.
        let _ = board_store::post_media::cancel_upload(
            &state.pool,
            &reservation.id,
            &reservation.capability,
        )
        .await;
        return Err(error);
    }
    if !matches!(multipart.next_field().await, Ok(None)) {
        let _ = board_store::post_media::cancel_upload(
            &state.pool,
            &reservation.id,
            &reservation.capability,
        )
        .await;
        return Err(invalid());
    }
    page(
        state,
        board_settings,
        UploadForm {
            upload_id: reservation.id,
            upload_capability: reservation.capability,
            resto,
        },
        UploadState::Queued,
        "Your file is queued for isolated processing. Check its status before posting.",
        format,
    )
}

pub async fn status(
    State(state): State<AppState>,
    Path(board): Path<String>,
    headers: HeaderMap,
    form: Result<Form<UploadForm>, FormRejection>,
) -> Response {
    let format = Format::from_headers(&headers);
    let result = async { current(&state, &board, parsed(form)?, format).await }.await;
    finish(&state, format, result)
}

async fn current(
    state: &AppState,
    board: &str,
    form: UploadForm,
    format: Format,
) -> Result<Response, AppError> {
    let board_settings = settings(state, board, form.resto).await?;
    board_store::post_media::check_upload(&state.pool, &form.upload_id, &form.upload_capability)
        .await?;
    let media = state.media.as_ref().ok_or_else(intake::unavailable)?;
    let status = media
        .status(&form.upload_id, &form.upload_capability)
        .await?;
    let (stage, message) = match status.state.as_str() {
        "published" if status.output_id.is_some() => (
            UploadState::Approved,
            "Your file is approved. Complete your post below.",
        ),
        "failed" => (
            UploadState::Failed,
            "Processing failed. Cancel this upload and choose another file.",
        ),
        "receiving" | "uploading" => (
            UploadState::Incomplete,
            "The upload is incomplete. Cancel it and choose the file again.",
        ),
        "published" => (
            UploadState::Failed,
            "No approved output is available. Cancel this upload and try another file.",
        ),
        "queued" => (
            UploadState::Queued,
            "Your file is still being processed. Check again shortly.",
        ),
        _ => (
            UploadState::Processing,
            "Your file is still being processed. Check again shortly.",
        ),
    };
    page(state, board_settings, form, stage, message, format)
}

pub async fn cancel(
    State(state): State<AppState>,
    Path(board): Path<String>,
    headers: HeaderMap,
    form: Result<Form<UploadForm>, FormRejection>,
) -> Response {
    let format = Format::from_headers(&headers);
    let result = async { revoke(&state, &board, parsed(form)?, format).await }.await;
    finish(&state, format, result)
}

async fn revoke(
    state: &AppState,
    board: &str,
    form: UploadForm,
    format: Format,
) -> Result<Response, AppError> {
    if state.media.is_none() {
        return Err(AppError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Media uploads are unavailable.",
        ));
    }
    board_store::board(&state.pool, board).await?;
    board_store::post_media::cancel_upload(&state.pool, &form.upload_id, &form.upload_capability)
        .await?;
    match format {
        Format::Html => Ok(Redirect::to(&format!("/{board}/")).into_response()),
        Format::Json => {
            #[derive(Serialize)]
            struct Cancelled {
                cancelled: bool,
            }
            json(state, &Cancelled { cancelled: true })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_receipt_keeps_exact_identifiers_without_extra_authority() {
        let value = serde_json::to_value(Receipt {
            upload_id: &"1".repeat(32),
            upload_capability: &"2".repeat(64),
            resto: i64::MAX.to_string(),
            state: UploadState::Approved,
        })
        .unwrap();
        assert_eq!(value["resto"], "9223372036854775807");
        assert_eq!(value["state"], "approved");
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert!(serde_json::to_vec(&value).unwrap().len() < 1024);
    }
}
