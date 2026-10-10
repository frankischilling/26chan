//! Published polls and bounded, cookie-bound votes. The supplied source includes
//! the form and results templates; token and duplicate rules are defined here.
use crate::{
    AppState,
    handlers::AppError,
    views::polls::{CataloguePage, OptionsPage, ResultsPage},
};
use axum::{
    Form, Router,
    extract::{
        DefaultBodyLimit, Path, State,
        rejection::{FormRejection, PathRejection},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use board_domain::poll_voting::{COOKIE_SECONDS, PollVoter, PollVotingKey};
use board_store::{PollSnapshot, PollVoteOutcome, StoreError};
use chrono::Datelike;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VoteForm {
    action: String,
    id: String,
    _ptkn: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/polls", get(catalogue))
        .route(
            "/polls/{id}",
            get(options).post(vote).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/polls/results/{id}", get(results))
}

async fn catalogue(State(state): State<AppState>) -> Result<Response, AppError> {
    let polls = board_store::poll_catalogue(&state.pool).await?;
    crate::output::html(
        &state,
        &CataloguePage {
            polls,
            copyright_year: chrono::Utc::now().year(),
        },
    )
}

fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Poll not found.")
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Poll voting is unavailable. Try again later.",
    )
}

fn invalid_token() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "This voting form has expired or is invalid. Reload the poll and try again.",
    )
}

fn positive_id(value: &str) -> Option<i64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse::<i64>().ok().filter(|id| *id > 0))
        .flatten()
}

fn voting_key(state: &AppState) -> Result<PollVotingKey, AppError> {
    state
        .poster_id_key
        .as_ref()
        .map(|key| key.poll_voting_key())
        .ok_or_else(unavailable)
}

async fn voting_ready(state: &AppState) -> Result<(), AppError> {
    for statement in [
        board_store::POLL_READINESS_SQL,
        board_store::POLL_VOTE_READINESS_SQL,
    ] {
        let ready: bool = sqlx::query_scalar(statement)
            .fetch_one(&state.pool)
            .await
            .map_err(StoreError::from)?;
        if !ready {
            return Err(unavailable());
        }
    }
    Ok(())
}

fn cookie_name(production: bool) -> &'static str {
    if production {
        "__Host-board-poll"
    } else {
        "board-poll"
    }
}

fn voter_cookie(
    headers: &HeaderMap,
    production: bool,
    key: &PollVotingKey,
    now: i64,
) -> Result<Option<PollVoter>, AppError> {
    let mut size = 0;
    let mut present = false;
    let mut voter = None;
    for part in headers.get_all(header::COOKIE) {
        size += part.as_bytes().len();
        if size > 8192 {
            return Err(AppError(StatusCode::BAD_REQUEST, "Invalid cookie header."));
        }
        let part = part
            .to_str()
            .map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid cookie header."))?;
        for pair in part.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name != cookie_name(production) {
                continue;
            }
            if present {
                return Err(AppError(StatusCode::BAD_REQUEST, "Ambiguous poll cookie."));
            }
            present = true;
            voter = key.parse_voter(value, now);
        }
    }
    Ok(voter)
}

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

fn append_voter(response: &mut Response, voter: &PollVoter, production: bool) {
    let secure = if production { "; Secure" } else { "" };
    let cookie = format!(
        "{}={}; Path=/; Max-Age={COOKIE_SECONDS}; HttpOnly; SameSite=Strict{secure}",
        cookie_name(production),
        voter.credential()
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).expect("canonical poll cookie"),
    );
}

async fn snapshot(state: &AppState, id: &str) -> Result<PollSnapshot, AppError> {
    // Reject signs, overflow and non-decimal identifiers before storage access.
    let id = positive_id(id).ok_or_else(not_found)?;
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
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(|_| not_found())?;
    let poll = snapshot(&state, &id).await?;
    // Historical polls remain read-only. HEAD must not create a browser identity.
    if !poll.accepting_votes || method == Method::HEAD {
        return crate::output::html(
            &state,
            &OptionsPage {
                poll,
                form_token: String::new(),
                copyright_year: chrono::Utc::now().year(),
            },
        );
    }
    voting_ready(&state).await?;
    let key = voting_key(&state)?;
    let now = chrono::Utc::now().timestamp();
    let existing = voter_cookie(&headers, state.production, &key, now)?;
    let minted = existing.is_none();
    let voter = match existing {
        Some(voter) => voter,
        None => key.generate_voter(now).map_err(|_| unavailable())?,
    };
    let hash = key.voter_hash(&voter, poll.id).map_err(|_| unavailable())?;
    if board_store::has_poll_vote(&state.pool, poll.id, &hash)
        .await
        .map_err(|error| match error {
            StoreError::NotFound => not_found(),
            other => AppError::from(other),
        })?
    {
        return render_results(&state, poll).map(private);
    }
    // An operator must supply scores before opening an imported result set.
    // Do not offer a form that the atomic vote function cannot accept.
    if poll.options.is_empty() || poll.options.iter().any(|option| option.score.is_none()) {
        return Err(unavailable());
    }
    let form_token = key
        .form_token(&voter, poll.id, now)
        .map_err(|_| unavailable())?;
    let mut response = private(crate::output::html(
        &state,
        &OptionsPage {
            poll,
            form_token,
            copyright_year: chrono::Utc::now().year(),
        },
    )?);
    if minted {
        append_voter(&mut response, &voter, state.production);
    }
    Ok(response)
}

async fn vote(
    State(state): State<AppState>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    form: Result<Form<VoteForm>, FormRejection>,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(|_| not_found())?;
    let poll = snapshot(&state, &id).await?;
    let Form(form) = form.map_err(|error| AppError(error.status(), "Invalid poll voting form."))?;
    let option_id = positive_id(&form.id)
        .filter(|_| form.action == "vote")
        .ok_or(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Choose a poll option.",
        ))?;
    let key = voting_key(&state)?;
    let now = chrono::Utc::now().timestamp();
    let voter = voter_cookie(&headers, state.production, &key, now)?.ok_or_else(invalid_token)?;
    if !key.verify_form_token(&voter, poll.id, &form._ptkn, now) {
        return Err(invalid_token());
    }
    voting_ready(&state).await?;
    let hash = key.voter_hash(&voter, poll.id).map_err(|_| unavailable())?;
    let outcome = board_store::cast_poll_vote(&state.pool, poll.id, option_id, &hash)
        .await
        .map_err(|error| match error {
            StoreError::NotFound => not_found(),
            other => AppError::from(other),
        })?;
    match outcome {
        PollVoteOutcome::Recorded | PollVoteOutcome::AlreadyVoted => Ok(private(
            Redirect::to(&format!("/polls/results/{}", poll.id)).into_response(),
        )),
        PollVoteOutcome::Closed => Err(AppError(StatusCode::CONFLICT, "This poll is closed.")),
        PollVoteOutcome::InvalidOption => Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Choose an available poll option.",
        )),
        PollVoteOutcome::CapacityReached => Err(AppError(
            StatusCode::CONFLICT,
            "This poll has reached its voting limit.",
        )),
    }
}

async fn results(
    State(state): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Response, AppError> {
    let Path(id) = id.map_err(|_| not_found())?;
    let poll = snapshot(&state, &id).await?;
    render_results(&state, poll)
}

fn render_results(state: &AppState, poll: PollSnapshot) -> Result<Response, AppError> {
    crate::output::html(state, &ResultsPage::new(poll, chrono::Utc::now().year()))
}
