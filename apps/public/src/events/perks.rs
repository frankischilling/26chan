//! Read-only entry point for the explicitly enabled score/perk reconstruction.
use crate::{AppState, handlers::AppError, verification_form::Fields};
use askama::Template;
use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use board_domain::source_events::{PERKS, perks};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/event-perks.html", get(page))
        .route("/event-like.html", get(find_post))
}
#[derive(Template)]
#[template(path = "event-perks.html")]
struct Page {
    active: bool,
    choices: Vec<perks::Perk>,
}
async fn page(State(state): State<AppState>) -> Result<Response, AppError> {
    let active = board_store::events::scores::active_run(&state.pool)
        .await?
        .is_some();
    let mut response = crate::output::html(
        &state,
        &Page {
            active,
            choices: if active {
                PERKS
                    .iter()
                    .filter_map(|name| perks::choose(name, 0, 0))
                    .collect()
            } else {
                Vec::new()
            },
        },
    )?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok(response)
}
async fn find_post(State(state): State<AppState>, request: Request) -> Result<Response, AppError> {
    let bad = || AppError(StatusCode::BAD_REQUEST, "Enter a board and a post number.");
    let fields = Fields::parse(
        request.uri().query().unwrap_or("").as_bytes(),
        &["board", "post"],
        256,
    )
    .map_err(|_| bad())?;
    let board = fields.get("board");
    board_domain::BoardSlug::parse(board).map_err(|_| bad())?;
    let post = fields.get("post").parse::<i64>().map_err(|_| bad())?;
    if post <= 0 || post.to_string() != fields.get("post") {
        return Err(bad());
    }
    if board_store::events::scores::active_run(&state.pool)
        .await?
        .is_none()
    {
        return Err(AppError(StatusCode::NOT_FOUND, "This event is closed."));
    }
    let mut response = Redirect::to(&format!("/event-like/{board}/{post}")).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok(response)
}
