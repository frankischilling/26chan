use crate::{AppState, api, catalog, views::*};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use askama::Template;
use axum::{
    Form,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
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
        concat!(
            include_str!("../static/board.css"),
            "\n",
            include_str!("../../../assets/comment-markup.css")
        ),
    )
}
pub async fn ready(State(state): State<AppState>) -> Result<&'static str, AppError> {
    sqlx::query("SELECT slug FROM content.boards LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if let Some(media) = &state.media {
        media.ready().await?;
    }
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
    board_page(&state, &board, 1, false, catalog::Options::default()).await
}
pub async fn page(
    State(state): State<AppState>,
    Path((board, page)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, AppError> {
    match page.as_str() {
        "catalog" => {
            let options = catalog::Options::parse(&uri).ok_or(AppError(
                StatusCode::BAD_REQUEST,
                "Invalid catalog options.",
            ))?;
            board_page(&state, &board, 1, true, options).await
        }
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
                board_page(
                    &state,
                    &board,
                    index + 1,
                    false,
                    catalog::Options::default(),
                )
                .await
            }
        }
    }
}
async fn board_page(
    state: &AppState,
    slug: &str,
    page: i64,
    catalog: bool,
    options: catalog::Options,
) -> Result<Response, AppError> {
    let selection = if catalog {
        board_store::BoardSelection::All
    } else {
        board_store::BoardSelection::Page(page)
    };
    let mut snapshot = board_store::board_snapshot(
        &state.pool,
        slug,
        selection,
        Some(if catalog { 0 } else { 3 }),
    )
    .await?;
    let hidden = if catalog {
        options.apply(&mut snapshot)
    } else {
        Vec::new()
    };
    let board = snapshot.board;
    let has_next = snapshot.has_next;
    let mut views = Vec::new();
    let mut hidden_views = Vec::new();
    for (preview, visible) in snapshot
        .threads
        .into_iter()
        .map(|preview| (preview, true))
        .chain(hidden.into_iter().map(|preview| (preview, false)))
    {
        let posts = preview.posts;
        if posts.is_empty() {
            continue;
        }
        let total = preview.visible_posts as usize;
        let omitted = total.saturating_sub(posts.len());
        let view = ThreadView {
            tail_size: 0,
            latest_reply_id: preview.latest_reply_id,
            thread: preview.thread,
            posts: posts.into_iter().map(PostView::new).collect(),
            omitted,
            image_replies: preview.visible_images,
        };
        if visible {
            views.push(view);
        } else {
            hidden_views.push(view);
        }
    }
    Ok(Html(
        BoardPage {
            catalog_hidden: hidden_views,
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
            catalog_options: options,
            media_origin: state
                .media
                .as_ref()
                .map(|m| m.settings.origin.as_string())
                .unwrap_or_default(),
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
        let tail = id.ends_with("-tail");
        let id = id.strip_suffix("-tail").unwrap_or(id);
        let raw = id;
        let id = id
            .parse::<i64>()
            .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
        if tail && (id <= 0 || id.to_string() != raw) {
            return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
        }
        return api::thread_selection(&state, &board, id, &headers, tail).await;
    }
    let id = key
        .parse()
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
        tail_size,
        images,
        ..
    } = board_store::thread_snapshot(&state.pool, &board, id).await?;
    let latest_reply_id = posts
        .iter()
        .filter(|post| post.id != thread.id)
        .map(|post| post.id)
        .max();
    let posts = posts.into_iter().map(PostView::new).collect();
    Ok(Html(
        BoardPage {
            catalog_hidden: Vec::new(),
            board,
            threads: vec![ThreadView {
                tail_size,
                latest_reply_id,
                thread,
                posts,
                omitted: 0,
                image_replies: images as i64,
            }],
            parent: id,
            previous: String::new(),
            next: String::new(),
            catalog: false,
            catalog_options: catalog::Options::default(),
            media_origin: state
                .media
                .as_ref()
                .map(|m| m.settings.origin.as_string())
                .unwrap_or_default(),
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
    #[serde(default)]
    com: String,
    #[serde(alias = "pwd")]
    password: String,
    #[serde(default)]
    resto: i64,
    #[serde(default)]
    email: String,
    #[serde(default)]
    upload_id: String,
    #[serde(default)]
    upload_capability: String,
    #[serde(default, deserialize_with = "crate::posting_form::checkbox")]
    spoiler: bool,
    #[serde(default)]
    awt: Option<u8>,
    #[serde(default)]
    track: Option<u8>,
    #[serde(default, rename = "mode")]
    _mode: Option<crate::posting_form::Mode>,
    // Source form hints carry no server authority or capacity override.
    #[serde(default, rename = "MAX_FILE_SIZE")]
    _max_file_size: String,
    #[serde(default, rename = "hasjs")]
    _hasjs: String,
    #[serde(default, deserialize_with = "crate::posting_form::checkbox")]
    textonly: bool,
}
pub async fn post(
    State(state): State<AppState>,
    Path(board): Path<String>,
    axum::Extension(start): axum::Extension<crate::security::RequestStart>,
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    form: Result<crate::posting_form::PostingForm, crate::posting_form::Rejection>,
) -> Response {
    let format = crate::posting_response::Format::from_headers(&headers);
    let response = match form {
        Ok(crate::posting_form::PostingForm(form)) => {
            match submit_post(
                state,
                board,
                headers,
                form,
                format,
                board_store::PostingContext {
                    request_start: start.0,
                    peer: peer.0,
                },
            )
            .await
            {
                Ok(response) => response,
                Err(error) => format.error(error),
            }
        }
        Err(error) => format.invalid_form(error),
    };
    format.finish(response)
}

async fn submit_post(
    state: AppState,
    board: String,
    headers: HeaderMap,
    form: PostForm,
    format: crate::posting_response::Format,
    context: board_store::PostingContext,
) -> Result<Response, AppError> {
    if state.production && context.peer.is_none() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Posting transport identity is unavailable.",
        ));
    }
    if [form.awt, form.track]
        .into_iter()
        .flatten()
        .any(|value| value > 1)
    {
        return Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Invalid posting preference.",
        ));
    }
    let auto_watch = form.awt == Some(1);
    let track = form.track == Some(1);
    let settings = board_store::board(&state.pool, &board).await?;
    let attachment = match (form.upload_id.is_empty(), form.upload_capability.is_empty()) {
        (true, true) if !form.spoiler => None,
        (false, false) if state.media.is_some() && !form.textonly => {
            Some(board_store::post_media::NewAttachment {
                upload: board_store::media_intake::IntakeReservation {
                    id: form.upload_id,
                    capability: form.upload_capability,
                },
                spoiler: form.spoiler,
            })
        }
        _ => {
            return Err(AppError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid or unavailable attachment.",
            ));
        }
    };
    settings.check_attachment_allowed(form.resto, attachment.is_some())?;
    board_domain::prepare_post_content(
        &form.name,
        &form.sub,
        &form.com,
        settings.max_comment_chars as usize,
        attachment.is_some(),
        settings.comment_spacing(),
        if form.resto == 0 {
            board_domain::PostKind::Thread {
                subject_required: settings.require_subject,
                text_only: settings.text_only,
            }
        } else {
            board_domain::PostKind::Reply
        },
    )
    .map_err(|e| AppError(StatusCode::UNPROCESSABLE_ENTITY, e.0))?;
    let options = board_domain::posting_options::parse(&form.email)
        .map_err(|error| AppError(StatusCode::UNPROCESSABLE_ENTITY, error.0))?;
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
        name: if options.anonymous {
            String::new()
        } else {
            form.name
        },
        subject: form.sub,
        comment: form.com,
        deletion_hash: hash,
        sage: options.sage,
    };
    let id = board_store::create_post_with_context(
        &state.pool,
        &board,
        form.resto,
        &post,
        attachment.as_ref(),
        context,
    )
    .await?;
    let thread = if form.resto == 0 { id } else { form.resto };
    let location = if options.return_to_board {
        format!("/{board}/")
    } else {
        format!("/{board}/thread/{thread}#p{id}")
    };
    let mut response = format.success(form.resto, id, &location);
    crate::post_receipts::Receipt {
        board: &board,
        thread,
        post: id,
        track,
        watch: auto_watch,
        production: state.production,
    }
    .append(response.headers_mut(), &headers);
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteForm {
    no: i64,
    password: String,
    #[serde(default)]
    file_only: bool,
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
    if form.file_only {
        board_store::post_media::delete_attachment(&state.pool, &board, form.no).await?;
    } else {
        board_store::delete_post(&state.pool, &board, form.no).await?;
    }
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
    Ok(Html(
        Message {
            title: "Report received",
            message: "Your report was saved.",
        }
        .render()?,
    ))
}
