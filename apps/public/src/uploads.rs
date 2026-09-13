use crate::{
    AppState,
    handlers::AppError,
    intake,
    views::{UploadForm, UploadPage},
};
use askama::Template;
use axum::{
    Form,
    body::Body,
    extract::{FromRequest, Multipart, Path, Request, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use bytes::Bytes;

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
    if resto > 0 {
        let thread = board_store::thread(&state.pool, board, resto).await?;
        if thread.archived_at.is_some()
            || thread.closed
            || thread.reply_count >= settings.reply_limit
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
    board: board_store::Board,
    form: UploadForm,
    ready: bool,
    message: &'static str,
) -> Result<Response, AppError> {
    Ok((
        [("cache-control", "private, no-store")],
        Html(
            UploadPage {
                board,
                form,
                ready,
                message,
            }
            .render()?,
        ),
    )
        .into_response())
}

pub async fn upload(
    State(state): State<AppState>,
    Path(board): Path<String>,
    request: Request,
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
    let mut multipart = Multipart::from_request(request, &state)
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
    let board_settings = settings(&state, &board, resto).await?;
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
        board_settings,
        UploadForm {
            upload_id: reservation.id,
            upload_capability: reservation.capability,
            resto,
        },
        false,
        "Your file is queued for isolated processing. Check its status before posting.",
    )
}

pub async fn status(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Form(form): Form<UploadForm>,
) -> Result<Response, AppError> {
    let board_settings = settings(&state, &board, form.resto).await?;
    board_store::post_media::check_upload(&state.pool, &form.upload_id, &form.upload_capability)
        .await?;
    let media = state.media.as_ref().ok_or_else(intake::unavailable)?;
    let status = media
        .status(&form.upload_id, &form.upload_capability)
        .await?;
    let ready = status.state == "published" && status.output_id.is_some();
    let message = match status.state.as_str() {
        "published" if ready => "Your file is approved. Complete your post below.",
        "failed" => "Processing failed. Cancel this upload and choose another file.",
        "receiving" | "uploading" => {
            "The upload is incomplete. Cancel it and choose the file again."
        }
        "published" => "No approved output is available. Cancel this upload and try another file.",
        _ => "Your file is still being processed. Check again shortly.",
    };
    page(board_settings, form, ready, message)
}

pub async fn cancel(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Form(form): Form<UploadForm>,
) -> Result<Redirect, AppError> {
    if state.media.is_none() {
        return Err(AppError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Media uploads are unavailable.",
        ));
    }
    board_store::board(&state.pool, &board).await?;
    board_store::post_media::cancel_upload(&state.pool, &form.upload_id, &form.upload_capability)
        .await?;
    Ok(Redirect::to(&format!("/{board}/")))
}
