//! Read-only published polls. Voting authority is outside this projection.
use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    Router,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
    response::Response,
    routing::get,
};
use board_store::{PollSnapshot, PollSummary, StoreError};

#[derive(Template)]
#[template(path = "polls.html")]
struct CataloguePage {
    polls: Vec<PollSummary>,
}

#[derive(Template)]
#[template(path = "poll-options.html")]
struct OptionsPage {
    poll: PollSnapshot,
}

struct ResultRow {
    caption: String,
    score: i64,
    percentage: String,
}

#[derive(Template)]
#[template(path = "poll-results.html")]
struct ResultsPage {
    id: i64,
    title: String,
    description: String,
    vote_count: i64,
    options: Vec<ResultRow>,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/polls", get(catalogue))
        .route("/polls/{id}", get(options))
        .route("/polls/results/{id}", get(results))
}

async fn catalogue(State(state): State<AppState>) -> Result<Response, AppError> {
    let polls = board_store::poll_catalogue(&state.pool).await?;
    crate::output::html(&state, &CataloguePage { polls })
}

fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Poll not found.")
}

async fn snapshot(state: &AppState, id: &str) -> Result<PollSnapshot, AppError> {
    // Reject signs, overflow and non-decimal identifiers before storage access.
    let id = if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
        id.parse::<i64>().ok().filter(|id| *id > 0)
    } else {
        None
    }
    .ok_or_else(not_found)?;
    board_store::poll_snapshot(&state.pool, id)
        .await
        .map_err(|error| match error {
            StoreError::NotFound => not_found(),
            other => AppError::from(other),
        })
}

async fn options(
    State(state): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(|_| not_found())?;
    let poll = snapshot(&state, &id).await?;
    crate::output::html(&state, &OptionsPage { poll })
}

async fn results(
    State(state): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(|_| not_found())?;
    let poll = snapshot(&state, &id).await?;
    let options = poll
        .options
        .into_iter()
        .map(|option| {
            let score = option.score.unwrap_or(0);
            ResultRow {
                caption: option.caption,
                score,
                percentage: percentage(score, poll.vote_count),
            }
        })
        .collect();
    crate::output::html(
        &state,
        &ResultsPage {
            id: poll.id,
            title: poll.title,
            description: poll.description,
            vote_count: poll.vote_count,
            options,
        },
    )
}

/// Presentation contract for stored counts in 0..=1_000_000_000: nearest
/// hundredth of a percent, positive half-ties rounded up, trailing zeros omitted.
/// Integer arithmetic avoids float-dependent output; this does not promise
/// arbitrary PHP numeric equivalence. A zero denominator displays zero percent.
fn percentage(score: i64, total: i64) -> String {
    if total == 0 {
        return "0".into();
    }
    let hundredths = (i128::from(score) * 10_000 + i128::from(total) / 2) / i128::from(total);
    let whole = hundredths / 100;
    match hundredths % 100 {
        0 => whole.to_string(),
        fraction if fraction % 10 == 0 => format!("{whole}.{}", fraction / 10),
        fraction => format!("{whole}.{fraction:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::percentage;

    #[test]
    fn bounded_percentages_round_and_omit_trailing_zeros() {
        for (score, total, expected) in [
            (0, 0, "0"),
            (1, 0, "0"),
            (0, 3, "0"),
            (1, 3, "33.33"),
            (2, 3, "66.67"),
            (1, 8, "12.5"),
            (1, 32, "3.13"),
            (1, 200, "0.5"),
            (1, 20_000, "0.01"),
            (1, 1_000_000_000, "0"),
            (1_000_000_000, 1_000_000_000, "100"),
            (999_999_999, 1_000_000_000, "100"),
            (1_000_000_000, 1, "100000000000"),
        ] {
            assert_eq!(percentage(score, total), expected);
        }
    }
}
