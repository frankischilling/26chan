//! Final dynamic output is allocated through the shared response block budget.
use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_http::{EncodedResponse, ResponseWriter};
use serde::Serialize;

pub(crate) fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Response exceeds the available output budget. Try again later.",
    )
}

pub(crate) fn html(state: &AppState, template: &impl Template) -> Result<Response, AppError> {
    html_writer(state.limits.response_writer(usize::MAX), template)
}

pub(crate) fn html_writer(
    mut writer: ResponseWriter,
    template: &impl Template,
) -> Result<Response, AppError> {
    template
        .render_into(&mut writer)
        .map_err(|_| unavailable())?;
    let encoded = writer.finish().map_err(|_| unavailable())?;
    Ok((
        [("content-type", "text/html; charset=utf-8")],
        encoded.into_body(),
    )
        .into_response())
}

pub(crate) fn json(
    mut writer: ResponseWriter,
    value: &impl Serialize,
) -> Result<EncodedResponse, AppError> {
    serde_json::to_writer(&mut writer, value).map_err(|_| unavailable())?;
    writer.finish().map_err(|_| unavailable())
}
