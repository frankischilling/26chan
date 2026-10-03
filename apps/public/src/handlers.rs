use crate::{AppState, api, catalog, views::*};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use askama::Template;
use axum::{
    Form,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
};
use board_store::{NewPost, StoreError};
use rand_core::OsRng;
use serde::Deserialize;
use sha2::{Digest, Sha256};

// The stored hash is not allowed to choose an unbounded verification workload.
// Public posting creates exactly this Argon2id profile; other encodings cannot
// authorize deletion or prove OP ownership. Keep this work inside the shared
// hash semaphore.
fn verify_deletion_password(password: &str, encoded: &str) -> bool {
    if encoded.len() > 256 {
        return false;
    }
    PasswordHash::new(encoded).is_ok_and(|hash| {
        hash.algorithm.as_str() == "argon2id"
            && hash.version == Some(19)
            && hash.params.iter().count() == 3
            && hash.params.get_decimal("m") == Some(19_456)
            && hash.params.get_decimal("t") == Some(2)
            && hash.params.get_decimal("p") == Some(1)
            && hash.hash.as_ref().is_some_and(|output| output.len() == 32)
            && Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
    })
}

#[derive(Debug)]
pub struct AppError(pub StatusCode, pub &'static str);
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        message_response(self.0, self.1)
    }
}
pub(crate) fn message_response(status: StatusCode, message: &str) -> Response {
    let body = Message {
        title: "Request could not be completed",
        message,
    }
    .render()
    .unwrap_or_else(|_| "Request failed.".into());
    (status, Html(body)).into_response()
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
            StoreError::AuthorizationChanged => {
                Self(StatusCode::FORBIDDEN, "Deletion password is invalid.")
            }
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
    sqlx::query("SELECT slug,meta_board,poster_id_no_heaven FROM content.boards LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    sqlx::query("SELECT json_op_poster_id FROM content.posts LIMIT 1")
        .execute(&state.pool)
        .await
        .map_err(StoreError::from)?;
    let attachment_policy: bool = sqlx::query_scalar("SELECT has_column_privilege('board_attachment_owner','content.boards','meta_board','SELECT') AND has_column_privilege('board_attachment_owner','content.boards','poster_id_no_heaven','SELECT')")
        .fetch_one(&state.pool)
        .await
        .map_err(StoreError::from)?;
    if !attachment_policy {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service unavailable.",
        ));
    }
    if let Some(media) = &state.media {
        media.ready().await?;
    }
    Ok("ready")
}
pub async fn home(State(state): State<AppState>) -> Result<Response, AppError> {
    crate::output::html(
        &state,
        &Home {
            boards: board_store::boards(&state.pool).await?,
        },
    )
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
        "index.rss" => crate::rss::feed(&state, &board, &headers).await,
        "archive" => {
            let board_store::PageSnapshot {
                snapshot,
                navigation_boards,
            } = board_store::archive_page_snapshot(&state.pool, &board).await?;
            crate::output::html(
                &state,
                &ArchivePage {
                    navigation_boards,
                    board: snapshot.board,
                    entries: snapshot.entries,
                },
            )
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
    let board_store::PageSnapshot {
        mut snapshot,
        navigation_boards,
    } = board_store::board_page_snapshot(
        &state.pool,
        slug,
        selection,
        Some(if catalog { 0 } else { 3 }),
    )
    .await?;
    if catalog && !snapshot.board.catalog_enabled {
        return Err(AppError(StatusCode::NOT_FOUND, "Catalog not found."));
    }
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
            catalog_last_reply: preview.catalog_last_reply,
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
    crate::output::html(
        state,
        &BoardPage {
            navigation_boards,
            quote: String::new(),
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
        },
    )
}
pub async fn thread(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
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
    let board_store::PageSnapshot {
        snapshot,
        navigation_boards,
    } = board_store::thread_page_snapshot(&state.pool, &board, id).await?;
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
        tail_size,
        images,
        ..
    } = snapshot;
    let latest_reply_id = posts
        .iter()
        .filter(|post| post.id != thread.id)
        .map(|post| post.id)
        .max();
    #[derive(Deserialize)]
    struct ReplyQuery {
        quote: Option<String>,
    }
    let query = Query::<ReplyQuery>::try_from_uri(&uri)
        .map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid reply target."))?;
    let quote = match query.quote.as_deref() {
        None => String::new(),
        Some(raw) => {
            let no = raw
                .parse::<i64>()
                .ok()
                .filter(|no| *no > 0 && no.to_string() == raw)
                .ok_or(AppError(StatusCode::BAD_REQUEST, "Invalid reply target."))?;
            if !posts.iter().any(|post| post.id == no) {
                return Err(AppError(StatusCode::NOT_FOUND, "Post not found."));
            }
            if thread.closed || thread.archived_at.is_some() {
                return Err(AppError(StatusCode::BAD_REQUEST, "This thread is closed."));
            }
            format!(">>{no}\n")
        }
    };
    let posts = posts.into_iter().map(PostView::new).collect();
    crate::output::html(
        &state,
        &BoardPage {
            navigation_boards,
            quote,
            catalog_hidden: Vec::new(),
            board,
            threads: vec![ThreadView {
                catalog_last_reply: None,
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
        },
    )
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
    flag: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    sub: String,
    #[serde(default)]
    com: String,
    #[serde(default, alias = "pwd")]
    password: String,
    #[serde(default)]
    resto: i64,
    #[serde(default)]
    email: String,
    #[serde(default)]
    upload_id: String,
    #[serde(default)]
    upload_capability: String,
    #[serde(default, deserialize_with = "crate::posting_form::spoiler_checkbox")]
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
                    op_password_proof: None,
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
    mut context: board_store::PostingContext,
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
        (true, true) => None,
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
    board_domain::prepare_post_content_input(
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
    if !form.password.is_empty() && !(8..=128).contains(&form.password.len()) {
        return Err(AppError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Deletion password must contain 8 to 128 bytes.",
        ));
    }
    let session =
        crate::anonymous_session::Session::resolve(&state, &headers, context.peer).await?;
    let password = session.password(&form.password);
    // The operator may enable OP markup while this request waits on the board
    // lock. Capture proof independently of the earlier policy snapshot.
    let op_hash = if form.resto > 0 {
        board_store::op_deletion_hash(&state.pool, &board, form.resto).await?
    } else {
        None
    };
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
    let (hash, op_password_proof) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let proof = op_hash
            .as_deref()
            .filter(|hash| {
                !form.password.is_empty() && verify_deletion_password(&form.password, hash)
            })
            .map(|hash| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())));
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| (hash.to_string(), proof))
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
    context.op_password_proof = op_password_proof;
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
    let id = board_store::create_post_with_anonymous_session(
        &state.pool,
        &board,
        form.resto,
        &post,
        attachment.as_ref(),
        board_store::AnonymousPostingContext {
            posting: context,
            session: session.posting,
        },
        board_store::PostMetadata {
            spoiler: form.spoiler,
            country_database: state.country_database.as_deref(),
            flag: &form.flag,
            options: &form.email,
            keys: board_store::PostIdentityKeys {
                tripcode: state.tripcode_key.as_deref(),
                poster_id: state.poster_id_key.as_deref(),
            },
        },
    )
    .await;
    let id = match id {
        Ok(id) => id,
        Err(StoreError::Robot9000Rejected(message) | StoreError::ContentRejected(message)) => {
            return Ok(format.rule_error(&message));
        }
        Err(StoreError::ContentQuiet { post: quiet }) => {
            let thread = if form.resto == 0 { quiet } else { form.resto };
            let location = if options.return_to_board {
                format!("/{board}/")
            } else {
                format!("/{board}/thread/{thread}#p{quiet}")
            };
            let mut response = format.success(form.resto, quiet, &location);
            session.append(response.headers_mut(), state.production);
            crate::post_preferences::append(
                response.headers_mut(),
                &headers,
                if settings.forced_anon {
                    None
                } else {
                    Some(&post.name)
                },
                if options.anonymous { "" } else { &form.email },
                state.production,
            );
            return Ok(response);
        }
        Err(error) => return Err(error.into()),
    };
    let thread = if form.resto == 0 { id } else { form.resto };
    let location = if options.return_to_board {
        format!("/{board}/")
    } else {
        format!("/{board}/thread/{thread}#p{id}")
    };
    let mut response = format.success(form.resto, id, &location);
    session.append(response.headers_mut(), state.production);
    crate::post_preferences::append(
        response.headers_mut(),
        &headers,
        if settings.forced_anon {
            None
        } else {
            Some(&post.name)
        },
        if options.anonymous { "" } else { &form.email },
        state.production,
    );
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
    pub(crate) no: i64,
    #[serde(default)]
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) file_only: bool,
}
pub async fn delete(
    State(state): State<AppState>,
    Path(board): Path<String>,
    headers: HeaderMap,
    Form(form): Form<DeleteForm>,
) -> Result<Redirect, AppError> {
    if let Some(capability) = crate::anonymous_session::Session::existing(&state, &headers).await? {
        let token = capability.storage_hash();
        if let Some(proof) =
            board_store::anonymous_session::post_proof(&state.pool, &token, &board, form.no).await?
        {
            board_store::delete_with_anonymous_proof(
                &state.pool,
                &board,
                form.no,
                token,
                proof,
                form.file_only,
            )
            .await?;
            return Ok(Redirect::to(&format!("/{board}/")));
        }
    }
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
    let proof = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_deletion_password(&form.password, &hash)
            .then(|| <[u8; 32]>::from(Sha256::digest(hash.as_bytes())))
    })
    .await
    .unwrap_or(None);
    let Some(proof) = proof else {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Deletion password is invalid.",
        ));
    };
    board_store::delete_with_password_proof(&state.pool, &board, form.no, proof, form.file_only)
        .await?;
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
    axum::Extension(peer): axum::Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    Form(form): Form<ReportForm>,
) -> Result<Response, AppError> {
    if state.production && peer.0.is_none() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Posting transport identity is unavailable.",
        ));
    }
    let session = crate::anonymous_session::Session::resolve(&state, &headers, peer.0).await?;
    board_store::report_with_anonymous_session(
        &state.pool,
        &board,
        form.no,
        &form.reason,
        Some(session.posting),
    )
    .await?;
    let mut response = Html(
        Message {
            title: "Report received",
            message: "Your report was saved.",
        }
        .render()?,
    )
    .into_response();
    session.append(response.headers_mut(), state.production);
    Ok(response)
}

#[cfg(test)]
mod op_password_tests {
    use super::*;

    #[test]
    fn op_password_checks_the_real_hash_and_rejects_unbounded_profiles() {
        let hash = Argon2::default()
            .hash_password(b"owned-op-password", &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string();
        assert!(verify_deletion_password("owned-op-password", &hash));
        assert!(!verify_deletion_password("different-password", &hash));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("m=19456", "m=4294967295")
        ));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("t=2", "t=4294967295")
        ));
        assert!(!verify_deletion_password(
            "owned-op-password",
            &hash.replace("argon2id", "argon2i")
        ));
        assert!(!verify_deletion_password("owned-op-password", "missing"));
    }
}
