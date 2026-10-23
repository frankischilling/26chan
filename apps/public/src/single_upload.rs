//! Bounded source multipart orchestration. Files remain opaque and stream to
//! intake; the public process waits for the existing isolated publication gate.
use crate::{
    AppState,
    handlers::AppError,
    posting_form::{MAX_FORM_FIELDS, MultipartFieldCount, TEXT_FIELDS},
    uploads,
};
use axum::{
    body::Body,
    extract::{FromRequest, Multipart, Request},
    http::{StatusCode, header},
};
use board_store::media_intake::IntakeReservation;
use std::{collections::BTreeSet, time::Duration};

pub(crate) fn bounded(request: Request, maximum: usize) -> Request {
    let (parts, body) = request.into_parts();
    Request::from_parts(
        parts,
        Body::new(http_body_util::Limited::new(body, maximum)),
    )
}
fn invalid() -> AppError {
    AppError(StatusCode::UNPROCESSABLE_ENTITY, "Invalid posting form.")
}
fn multipart(request: &Request) -> bool {
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("multipart/form-data"))
}
pub(crate) async fn normalize(
    state: &AppState,
    board: &str,
    request: Request,
) -> Result<(Request, Option<IntakeReservation>), AppError> {
    if board != "f" || state.media.is_none() || !multipart(&request) {
        return Ok((bounded(request, 262_144), None));
    }
    let mut _permit = None;
    let (parts, body) = request.into_parts();
    let uri = parts.uri.clone();
    let mut reader = Multipart::from_request(Request::from_parts(parts, body), state)
        .await
        .map_err(|e| AppError(e.status(), "Invalid posting form."))?;
    let mut reservation = None;
    let result = async {
        let mut fields = Vec::<(String, String)>::new();
        let mut names = BTreeSet::new();
        let mut text_bytes = 0usize;
        while let Some(mut field) = reader
            .next_field()
            .await
            .map_err(|e| AppError(e.status(), "Invalid posting form."))?
        {
            let name = field
                .name()
                .filter(|name| name.len() <= 64)
                .ok_or_else(invalid)?
                .to_owned();
            let deletion =
                name.len() <= 19 && !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit());
            if !(TEXT_FIELDS.contains(&name.as_str())
                || name == "upfile"
                || deletion
                || matches!(
                    name.as_str(),
                    "onlyimgdel" | "res" | "board" | "no" | "cat" | "cat_id" | "revision"
                ))
                || !names.insert(name.clone())
                || names.len() > MAX_FORM_FIELDS
            {
                return Err(invalid());
            }
            if name == "upfile" {
                if field.file_name().is_none_or(str::is_empty) {
                    while let Some(chunk) = field.chunk().await.map_err(|_| invalid())? {
                        if !chunk.is_empty() {
                            return Err(invalid());
                        }
                    }
                    continue;
                }
                if names.contains("upload_id") || names.contains("upload_capability") {
                    return Err(invalid());
                }
                _permit = Some(state.limits.uploads.try_acquire().map_err(|_| {
                    AppError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Uploads are busy. Try again later.",
                    )
                })?);
                let settings = uploads::settings(state, board, 0).await?;
                if !settings.upload_board {
                    return Err(invalid());
                }
                reservation = Some(uploads::receive_file(state, &settings, 0, false, field).await?);
                continue;
            }
            if field.file_name().is_some()
                || (reservation.is_some()
                    && matches!(name.as_str(), "upload_id" | "upload_capability"))
            {
                return Err(invalid());
            }
            let mut value = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|e| AppError(e.status(), "Invalid posting form."))?
            {
                text_bytes = text_bytes.checked_add(chunk.len()).ok_or_else(invalid)?;
                if text_bytes > 262_144 {
                    return Err(AppError(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Posting form is too large.",
                    ));
                }
                value.extend_from_slice(&chunk);
            }
            fields.push((name, String::from_utf8(value).map_err(|_| invalid())?));
        }
        let original_count = names.len();
        if let Some(upload) = &reservation {
            if fields.len() + 2 > MAX_FORM_FIELDS {
                return Err(invalid());
            }
            fields.push(("upload_id".into(), upload.id.clone()));
            fields.push(("upload_capability".into(), upload.capability.clone()));
        }
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&fields)
            .finish();
        if encoded.len() > 3 * 262_144 {
            return Err(invalid());
        }
        let mut request = Request::post(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(encoded))
            .map_err(|_| invalid())?;
        request
            .extensions_mut()
            .insert(MultipartFieldCount(original_count));
        Ok(request)
    }
    .await;
    match result {
        Ok(request) => Ok((request, reservation)),
        Err(error) => {
            if let Some(upload) = &reservation {
                cancel(state, upload).await;
            }
            Err(error)
        }
    }
}
pub(crate) async fn cancel(state: &AppState, upload: &IntakeReservation) {
    let _ =
        board_store::post_media::cancel_upload(&state.pool, &upload.id, &upload.capability).await;
}
pub(crate) async fn approval(
    state: &AppState,
    upload: &IntakeReservation,
    remaining: Duration,
) -> Result<(), AppError> {
    let media = state.media.as_ref().ok_or_else(invalid)?;
    let result = tokio::time::timeout(remaining.min(Duration::from_secs(60)), async {
        loop {
            let status = media.status(&upload.id, &upload.capability).await?;
            match status.state.as_str() {
                "published" if status.output_id.is_some() => return Ok(()),
                "queued" | "processing" => tokio::time::sleep(Duration::from_millis(250)).await,
                _ => {
                    return Err(AppError(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "Error: Upload failed.",
                    ));
                }
            }
        }
    })
    .await;
    result.unwrap_or(Err(AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "File processing did not finish in time. Please try again later.",
    )))
}
