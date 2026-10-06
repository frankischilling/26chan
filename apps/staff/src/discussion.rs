use crate::{AppError, AppState, access::Level, auth, handlers::StaffRequestStart};
use askama::Template;
use axum::{
    Extension, Form,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use board_domain::{Line, Token, comment_markup::Tag, word_break::WordPart};
use board_store::{Board, BoardSelection, Post, StoreError};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

type Shared = State<Arc<AppState>>;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewQuery {
    quote: Option<i64>,
}

struct DiscussionPost {
    post: Post,
    lines: Vec<Line>,
    role: Option<Level>,
    now: String,
}

impl DiscussionPost {
    fn name_class(&self) -> &'static str {
        match self.role {
            Some(Level::Janitor) => "discussionJanitor",
            Some(Level::Moderator) => "discussionModerator",
            Some(Level::Manager) => "discussionManager",
            Some(Level::Admin) => "discussionAdmin",
            None => "discussionUnknown",
        }
    }
}

struct DiscussionThread {
    thread: board_store::Thread,
    posts: Vec<DiscussionPost>,
    omitted: i64,
}

#[derive(Template)]
#[template(path = "discussion.html")]
struct DiscussionPage {
    board: Board,
    threads: Vec<DiscussionThread>,
    parent: i64,
    csrf: String,
    comment: String,
    subject: String,
    recent: bool,
    closed: bool,
    previous: String,
    next: String,
    public_origin: String,
    error: String,
    field_bytes: usize,
    comment_max_units: usize,
}

fn store_error(error: StoreError) -> AppError {
    match error {
        StoreError::NotFound | StoreError::PageNotFound => AppError::NotFound,
        StoreError::Invalid(_) => AppError::Invalid,
        StoreError::Conflict(_) => AppError::Invalid,
        StoreError::AuthorizationChanged => AppError::Unauthorized,
        StoreError::Database(error) => AppError::Database(error),
        StoreError::UnsafeRole => AppError::Forbidden,
        StoreError::RandomnessUnavailable => AppError::Internal,
        StoreError::ReadLimit => AppError::Capacity,
        StoreError::Robot9000Rejected(_)
        | StoreError::ContentRejected(_)
        | StoreError::PublicDeletionRejected(_)
        | StoreError::ContentQuiet { .. } => AppError::Internal,
    }
}

fn html(state: &AppState, page: &DiscussionPage) -> Result<Response, AppError> {
    let mut output = state.limits.response_output.writer(64 * 1024 * 1024);
    page.render_into(&mut output)
        .map_err(|_| AppError::Capacity)?;
    let encoded = output.finish().map_err(|_| AppError::Capacity)?;
    Ok((
        [("content-type", "text/html; charset=utf-8")],
        encoded.into_body(),
    )
        .into_response())
}

fn page_link(page: i64) -> String {
    if page <= 1 {
        "/j/".into()
    } else {
        format!("/j/{}.php", page - 1)
    }
}

fn legacy_number(value: &str) -> Result<i64, AppError> {
    let value = value.strip_suffix(".php").unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::NotFound);
    }
    value.parse().map_err(|_| AppError::NotFound)
}

fn check_private(board: &Board) -> Result<(), AppError> {
    if board.slug != "j" || !board.staff_only {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn build_page(
    state: &AppState,
    headers: &HeaderMap,
    parent: i64,
    page: i64,
    query: ViewQuery,
) -> Result<DiscussionPage, AppError> {
    let mut authority = auth::guard(state, headers).await?;
    if !authority
        .session
        .permissions
        .can_discuss(&authority.session.role)
    {
        return Err(AppError::Forbidden);
    }
    let csrf = auth::cookie(headers, &format!("{}-csrf", state.config.cookie_name()))?;
    auth::csrf(&authority.session, &csrf)?;
    let (board, threads, has_next, closed) = if parent == 0 {
        let snapshot =
            board_store::board_snapshot(&state.staff, "j", BoardSelection::Page(page), Some(5))
                .await
                .map_err(store_error)?;
        check_private(&snapshot.board)?;
        let threads = snapshot
            .threads
            .into_iter()
            .map(|preview| {
                let omitted = preview
                    .visible_posts
                    .saturating_sub(preview.posts.len() as i64);
                (preview.thread, preview.posts, omitted)
            })
            .collect::<Vec<_>>();
        (snapshot.board, threads, snapshot.has_next, false)
    } else {
        let snapshot = board_store::thread_snapshot(&state.staff, "j", parent)
            .await
            .map_err(store_error)?;
        check_private(&snapshot.board)?;
        let closed = snapshot.thread.closed || snapshot.thread.archived_at.is_some();
        (
            snapshot.board,
            vec![(snapshot.thread, snapshot.posts, 0)],
            false,
            closed,
        )
    };
    let ids = threads
        .iter()
        .flat_map(|(_, posts, _)| posts.iter().map(|post| post.id))
        .collect::<Vec<_>>();
    let authors: BTreeMap<i64, String> = sqlx::query_as::<_, (i64, String)>(
        "SELECT d.post_id,a.role FROM staff_identity.discussion_posts d \
         JOIN staff_identity.accounts a ON a.id=d.account_id \
         WHERE d.post_id=ANY($1) AND cardinality(a.allow_boards)>0",
    )
    .bind(&ids)
    .fetch_all(authority.connection())
    .await?
    .into_iter()
    .collect();
    let quote = query.quote.filter(|id| *id > 0);
    let comment = if let Some(quote) = quote {
        let post = board_store::find_post(&state.staff, "j", quote)
            .await
            .map_err(store_error)?;
        if parent > 0 && post.thread_id != parent {
            return Err(AppError::NotFound);
        }
        format!(">>{quote}\n")
    } else {
        String::new()
    };
    let threads = threads
        .into_iter()
        .map(|(thread, posts, omitted)| DiscussionThread {
            thread,
            posts: posts
                .into_iter()
                .map(|post| {
                    let role = authors.get(&post.id).and_then(|role| Level::parse(role));
                    let lines = post.formatted_lines();
                    let now = post
                        .created_at
                        .with_timezone(&chrono_tz::America::New_York)
                        .format("%m/%d/%y(%a)%H:%M:%S")
                        .to_string();
                    DiscussionPost {
                        post,
                        lines,
                        role,
                        now,
                    }
                })
                .collect(),
            omitted,
        })
        .collect();
    authority.ensure_current(false).await?;
    let session = authority.finish().await?;
    let authorized_limits = session.at_least(Level::Moderator);
    let field_bytes = if authorized_limits {
        board_domain::MAX_AUTHORIZED_FIELD_BYTES
    } else {
        board_domain::MAX_PUBLIC_FIELD_BYTES
    };
    let comment_max_units = if authorized_limits {
        board.max_authorized_comment_chars
    } else {
        board.max_comment_chars
    } as usize
        * 2;
    Ok(DiscussionPage {
        board,
        threads,
        parent,
        csrf,
        comment,
        subject: String::new(),
        recent: session.recent,
        closed,
        previous: if parent == 0 && page > 1 {
            page_link(page - 1)
        } else {
            String::new()
        },
        next: if parent == 0 && has_next {
            page_link(page + 1)
        } else {
            String::new()
        },
        public_origin: state.config.public_origin.clone(),
        error: String::new(),
        field_bytes,
        comment_max_units,
    })
}

pub(crate) async fn index(
    State(state): Shared,
    headers: HeaderMap,
    Query(query): Query<ViewQuery>,
) -> Result<Response, AppError> {
    html(&state, &build_page(&state, &headers, 0, 1, query).await?)
}

pub(crate) async fn page(
    State(state): Shared,
    headers: HeaderMap,
    Path(value): Path<String>,
    Query(query): Query<ViewQuery>,
) -> Result<Response, AppError> {
    let page = legacy_number(&value)?
        .checked_add(1)
        .ok_or(AppError::NotFound)?;
    html(&state, &build_page(&state, &headers, 0, page, query).await?)
}

pub(crate) async fn thread(
    State(state): Shared,
    headers: HeaderMap,
    Path(thread): Path<i64>,
    Query(query): Query<ViewQuery>,
) -> Result<Response, AppError> {
    if thread <= 0 {
        return Err(AppError::NotFound);
    }
    html(
        &state,
        &build_page(&state, &headers, thread, 1, query).await?,
    )
}

pub(crate) async fn legacy_thread(
    State(state): Shared,
    headers: HeaderMap,
    Path(value): Path<String>,
    Query(query): Query<ViewQuery>,
) -> Result<Response, AppError> {
    let thread = legacy_number(&value)?;
    if thread <= 0 {
        return Err(AppError::NotFound);
    }
    html(
        &state,
        &build_page(&state, &headers, thread, 1, query).await?,
    )
}

pub(crate) async fn post_link(
    State(state): Shared,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Redirect, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    if !authority
        .session
        .permissions
        .can_discuss(&authority.session.role)
    {
        return Err(AppError::Forbidden);
    }
    let post = board_store::find_post(&state.staff, "j", id)
        .await
        .map_err(store_error)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Redirect::to(&format!(
        "/j/thread/{}#p{}",
        post.thread_id, post.id
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PostForm {
    csrf: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    resto: i64,
    #[serde(default)]
    sub: String,
    #[serde(default)]
    com: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
}

pub(crate) async fn submit(
    State(state): Shared,
    Extension(start): Extension<StaffRequestStart>,
    headers: HeaderMap,
    Form(input): Form<PostForm>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let session = auth::session(&state, &headers).await?;
    auth::csrf(&session, &input.csrf)?;
    if !session.permissions.can_discuss(&session.role) {
        return Err(AppError::Forbidden);
    }
    if !session.recent {
        return Err(AppError::Recent);
    }
    let authorized_limits = session.at_least(Level::Moderator);
    let field_bytes = if authorized_limits {
        board_domain::MAX_AUTHORIZED_FIELD_BYTES
    } else {
        board_domain::MAX_PUBLIC_FIELD_BYTES
    };
    if (!input.mode.is_empty() && input.mode != "regist")
        || input.resto < 0
        || input.name.len() > field_bytes
        || input.email.len() > field_bytes
    {
        return Err(AppError::Invalid);
    }
    let secret = auth::cookie(&headers, state.config.cookie_name())?;
    let session_hash = auth::hash(&secret);
    let csrf_hash = auth::hash(&input.csrf);
    let ticket_hash: [u8; 32] = auth::hash(&auth::token())
        .try_into()
        .map_err(|_| AppError::Internal)?;
    let board = board_store::board(&state.staff, "j")
        .await
        .map_err(store_error)?;
    check_private(&board)?;
    let result = board_store::create_staff_post(
        &state.staff,
        "j",
        input.resto,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: input.sub.clone(),
            comment: input.com.clone(),
            deletion_hash: String::new(),
            sage: false,
        },
        start.0,
        board_store::StaffPostAuthority {
            auth_pool: &state.auth,
            session_hash: &session_hash,
            csrf_hash: &csrf_hash,
            ticket_hash: &ticket_hash,
            idle_seconds: state.config.idle_timeout.as_secs() as i32,
            highlight: false,
            authorized_limits,
            identity: None,
        },
    )
    .await;
    match result {
        Ok(id) => {
            let parent = if input.resto == 0 { id } else { input.resto };
            Ok(Redirect::to(&format!("/j/thread/{parent}#p{id}")).into_response())
        }
        Err(StoreError::Invalid(message) | StoreError::Conflict(message)) => {
            let mut page =
                build_page(&state, &headers, input.resto, 1, ViewQuery::default()).await?;
            page.subject = input.sub;
            page.comment = input.com;
            page.error = message.into();
            let mut response = html(&state, &page)?;
            *response.status_mut() = StatusCode::BAD_REQUEST;
            Ok(response)
        }
        Err(error) => Err(store_error(error)),
    }
}

pub(crate) async fn stylesheet() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        concat!(
            include_str!("../../public/static/board.css"),
            "\n",
            include_str!("../../../assets/comment-markup.css"),
            "\n",
            include_str!("../../../assets/comment-markup-mobile.css"),
            "\n",
            include_str!("../static/discussion.css"),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_source_page_numbers_are_bounded_and_keep_php_aliases() {
        assert_eq!(legacy_number("0.php").unwrap(), 0);
        assert_eq!(legacy_number("13.php").unwrap(), 13);
        assert_eq!(page_link(1), "/j/");
        assert_eq!(page_link(3), "/j/2.php");
        for value in [
            "",
            "../j",
            "1.html",
            "-1",
            "9223372036854775808",
            "1.php.php",
        ] {
            assert!(legacy_number(value).is_err(), "{value}");
        }
    }
}
