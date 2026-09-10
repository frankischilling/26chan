use crate::{AppState, api, views::*};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use askama::Template;
use axum::{
    Form,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use board_store::{NewPost, StoreError};
use rand_core::OsRng;
use serde::Deserialize;

#[derive(Debug)]
pub struct AppError(pub StatusCode, pub &'static str);
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Message {
            title: "Request could not be completed",
            message: self.1,
        }
        .render()
        .unwrap_or_else(|_| "Request failed.".into());
        (self.0, Html(body)).into_response()
    }
}
impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::PageNotFound => Self(StatusCode::NOT_FOUND, "Page not found."),
            StoreError::NotFound => {
                Self(StatusCode::NOT_FOUND, "Board, thread, or post not found.")
            }
            StoreError::Invalid(message) => Self(StatusCode::UNPROCESSABLE_ENTITY, message),
            StoreError::Conflict(message) => Self(StatusCode::CONFLICT, message),
            _ => {
                tracing::warn!(
                    event = "database_operation_failed",
                    "database operation failed"
                );
                Self(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Storage is unavailable. Try again later.",
                )
            }
        }
    }
}
impl From<askama::Error> for AppError {
    fn from(_: askama::Error) -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Page could not be rendered.",
        )
    }
}
pub async fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Page not found.")
}
pub async fn css() -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "public, max-age=0, must-revalidate"),
        ],
        include_str!("../static/board.css"),
    )
}
pub async fn ready(State(state): State<AppState>) -> Result<&'static str, AppError> {
    sqlx::query("SELECT slug FROM content.boards LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    Ok("ready")
}
pub async fn home(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    Ok(Html(
        Home {
            boards: board_store::boards(&state.pool).await?,
        }
        .render()?,
    ))
}
pub async fn board_redirect(
    State(state): State<AppState>,
    Path(board): Path<String>,
) -> Result<Redirect, AppError> {
    board_store::board(&state.pool, &board).await?;
    Ok(Redirect::permanent(&format!("/{board}/")))
}
pub async fn board_index(
    State(state): State<AppState>,
    Path(board): Path<String>,
) -> Result<Response, AppError> {
    board_page(&state, &board, 1, false).await
}
pub async fn page(
    State(state): State<AppState>,
    Path((board, page)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    match page.as_str() {
        "catalog" => board_page(&state, &board, 1, true).await,
        "threads.json" => api::thread_list(&state, &board, &headers).await,
        "catalog.json" => api::catalog(&state, &board, &headers).await,
        "archive.json" => api::archive(&state, &board, &headers).await,
        "archive" => {
            let snapshot = board_store::archive_snapshot(&state.pool, &board).await?;
            Ok(Html(
                ArchivePage {
                    board: snapshot.board,
                    entries: snapshot.entries,
                }
                .render()?,
            )
            .into_response())
        }
        _ => {
            if let Some(index) = page.strip_suffix(".json") {
                let index = index
                    .parse()
                    .map_err(|_| AppError(StatusCode::NOT_FOUND, "Page not found."))?;
                api::index(&state, &board, index, &headers).await
            } else {
                let index = page
                    .parse::<i64>()
                    .map_err(|_| AppError(StatusCode::NOT_FOUND, "Page not found."))?;
                if !(0..1000).contains(&index) {
                    return Err(AppError(StatusCode::NOT_FOUND, "Page not found."));
                }
                board_page(&state, &board, index + 1, false).await
            }
        }
    }
}
async fn board_page(
    state: &AppState,
    slug: &str,
    page: i64,
    catalog: bool,
) -> Result<Response, AppError> {
    let selection = if catalog {
        board_store::BoardSelection::All
    } else {
        board_store::BoardSelection::Page(page)
    };
    let snapshot = board_store::board_snapshot(
        &state.pool,
        slug,
        selection,
        Some(if catalog { 0 } else { 3 }),
    )
    .await?;
    let board = snapshot.board;
    let has_next = snapshot.has_next;
    let mut views = Vec::new();
    for preview in snapshot.threads {
        let posts = preview.posts;
        if posts.is_empty() {
            continue;
        }
        let total = preview.visible_posts as usize;
        let omitted = total.saturating_sub(posts.len());
        views.push(ThreadView {
            thread: preview.thread,
            posts: posts.into_iter().map(PostView::new).collect(),
            omitted,
        });
    }
    Ok(Html(
        BoardPage {
            board,
            threads: views,
            parent: 0,
            previous: if page > 1 {
                format!("/{slug}/{}", page - 2)
            } else {
                String::new()
            },
            next: if has_next {
                format!("/{slug}/{page}")
            } else {
                String::new()
            },
            catalog,
        }
        .render()?,
    )
    .into_response())
}
pub async fn thread(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(id) = key.strip_suffix(".json") {
        let id = id
            .parse()
            .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
        return api::thread(&state, &board, id, &headers).await;
    }
    let id = key
        .parse()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
    } = board_store::thread_snapshot(&state.pool, &board, id).await?;
    let posts = posts.into_iter().map(PostView::new).collect();
    Ok(Html(
        BoardPage {
            board,
            threads: vec![ThreadView {
                thread,
                posts,
                omitted: 0,
            }],
            parent: id,
            previous: String::new(),
            next: String::new(),
            catalog: false,
        }
        .render()?,
    )
    .into_response())
}
pub async fn quote(
    State(state): State<AppState>,
    Path((board, id)): Path<(String, i64)>,
) -> Result<Redirect, AppError> {
    let post = board_store::find_post(&state.pool, &board, id).await?;
    Ok(Redirect::to(&format!(
        "/{board}/thread/{}#p{id}",
        post.thread_id
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    sub: String,
    com: String,
    password: String,
    #[serde(default)]
    resto: i64,
    #[serde(default)]
    email: String,
}
pub async fn post(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Form(form): Form<PostForm>,
) -> Result<Redirect, AppError> {
    let settings = board_store::board(&state.pool, &board).await?;
    board_domain::validate_post(
        &form.name,
        &form.sub,
        &form.com,
        settings.max_comment_chars as usize,
    )
    .map_err(|e| AppError(StatusCode::UNPROCESSABLE_ENTITY, e.0))?;
    let (sage, return_to_board) = match form.email.as_str() {
        "" => (false, false),
        "sage" => (true, false),
        "nonoko" => (false, true),
        "nonokosage" => (true, true),
        _ => {
            return Err(AppError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Options may be empty, sage, nonoko, or nonokosage.",
            ));
        }
    };
    if !(8..=128).contains(&form.password.len()) {
        return Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Deletion password must contain 8 to 128 bytes.",
        ));
    }
    let permit = state
        .limits
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Password processing is busy. Try again.",
            )
        })?;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(form.password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
    })
    .await
    .map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Password processing failed.",
        )
    })?
    .map_err(|_| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Password processing failed.",
        )
    })?;
    let post = NewPost {
        name: form.name,
        subject: if form.resto == 0 {
            form.sub
        } else {
            String::new()
        },
        comment: form.com,
        deletion_hash: hash,
        sage,
    };
    let id = board_store::create_post(&state.pool, &board, form.resto, &post).await?;
    if return_to_board {
        return Ok(Redirect::to(&format!("/{board}/")));
    }
    let thread = if form.resto == 0 { id } else { form.resto };
    Ok(Redirect::to(&format!("/{board}/thread/{thread}#p{id}")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteForm {
    no: i64,
    password: String,
}
pub async fn delete(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Result<Redirect, AppError> {
    if !(8..=128).contains(&form.password.len()) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Deletion password is invalid.",
        ));
    }
    let hash = board_store::deletion_hash(&state.pool, &board, form.no).await?;
    let permit = state
        .limits
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Password processing is busy. Try again.",
            )
        })?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(form.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Deletion password is invalid.",
        ));
    }
    board_store::delete_post(&state.pool, &board, form.no).await?;
    Ok(Redirect::to(&format!("/{board}/")))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportForm {
    no: i64,
    reason: String,
}
pub async fn report(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Form(form): Form<ReportForm>,
) -> Result<Html<String>, AppError> {
    board_store::report(&state.pool, &board, form.no, &form.reason).await?;
    Ok(Html(Message { title: "Report received", message: "Your report was saved. Staff review is not available in this development build." }.render()?))
}
