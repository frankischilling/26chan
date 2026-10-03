use crate::{AppError, AppState, auth, store, views};
use askama::Template;
use axum::{
    Extension, Form, Json,
    extract::{Query, Request, State},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use webauthn_rs::prelude::*;
type Shared = State<Arc<AppState>>;
#[derive(Clone, Copy)]
pub struct StaffRequestStart(pub chrono::DateTime<chrono::Utc>);

#[derive(Clone, Copy)]
pub struct StaffRequestPeer(Option<std::net::IpAddr>);

impl StaffRequestPeer {
    /// Canonical identity resolved by the listener and verified proxy policy.
    /// An embedded router without a listener identity returns no peer.
    pub fn ip(self) -> Option<std::net::IpAddr> {
        self.0
    }
}

pub async fn request_limits(State(state): Shared, mut request: Request, next: Next) -> Response {
    request
        .extensions_mut()
        .insert(StaffRequestStart(chrono::Utc::now()));
    let peer = match board_http::proxy_peer::resolve(
        &request,
        state.config.proxy.as_ref().map(|proxy| proxy.uid()),
    ) {
        Ok(peer) => peer,
        Err(status) => return status.into_response(),
    };
    request.extensions_mut().insert(StaffRequestPeer(peer));
    let Ok(permit) = state.limits.permits.clone().try_acquire_owned() else {
        return AppError::Capacity.into_response();
    };
    let response = request_limits_inner(state, request, next).await;
    board_http::hold_permit(response, permit)
}

async fn request_limits_inner(state: Arc<AppState>, request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::POST
        && matches!(request.uri().path(), "/login/start" | "/enroll/start")
    {
        let Ok(mut rate) = state.limits.attempts.lock() else {
            return AppError::Internal.into_response();
        };
        if rate.0.elapsed() >= std::time::Duration::from_secs(60) {
            *rate = (std::time::Instant::now(), 0);
        }
        if rate.1 >= 30 {
            return AppError::Capacity.into_response();
        }
        rate.1 += 1;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => AppError::Internal.into_response(),
    }
}

pub async fn security_headers(State(state): Shared, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key, value) in [
        ("cache-control", "private, no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
        (
            "permissions-policy",
            "publickey-credentials-get=(self), publickey-credentials-create=(self)",
        ),
    ] {
        response
            .headers_mut()
            .insert(key, HeaderValue::from_static(value));
    }
    let policy = format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'; connect-src 'self'; img-src {}",
        state.config.media_origin
    );
    let Ok(policy) = HeaderValue::from_str(&policy) else {
        return AppError::Internal.into_response();
    };
    response
        .headers_mut()
        .insert("content-security-policy", policy);
    response
}
pub async fn landing() -> Result<Html<String>, AppError> {
    Ok(Html(views::Login.render().map_err(|_| AppError::Internal)?))
}
pub async fn javascript() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../static/staff.js"),
    )
}
pub async fn post_limits_javascript() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../static/post-limits.js"),
    )
}
pub async fn ready(State(state): Shared) -> Result<&'static str, AppError> {
    auth::check_identity(&state.auth, "board_auth").await?;
    auth::check_identity(&state.staff, "board_staff").await?;
    sqlx::query("SELECT token_hash,last_activity_at FROM staff_identity.sessions LIMIT 0")
        .execute(&state.auth)
        .await?;
    sqlx::query("SELECT id FROM content.moderation_audit LIMIT 0")
        .execute(&state.staff)
        .await?;
    sqlx::query("SELECT post_id,available FROM content.staff_post_media LIMIT 0")
        .execute(&state.staff)
        .await?;
    Ok("ready")
}

pub async fn comment_css() -> impl IntoResponse {
    (
        [
            ("content-type", "text/css; charset=utf-8"),
            ("cache-control", "public, max-age=0, must-revalidate"),
        ],
        concat!(
            include_str!("../../../assets/comment-markup.css"),
            "\n",
            include_str!("../../../assets/comment-markup-mobile.css")
        ),
    )
}
fn set_cookie(
    response: &mut Response,
    state: &AppState,
    name: &str,
    value: &str,
    age: u32,
) -> Result<(), AppError> {
    response.headers_mut().append(
        "set-cookie",
        HeaderValue::from_str(&state.config.cookie(name, value, age))
            .map_err(|_| AppError::Internal)?,
    );
    Ok(())
}
fn ceremony_cookie(state: &AppState) -> String {
    format!("{}-ceremony", state.config.cookie_name())
}
fn csrf_cookie(state: &AppState) -> String {
    format!("{}-csrf", state.config.cookie_name())
}
#[derive(sqlx::FromRow)]
struct Account {
    id: i64,
    username: String,
    user_handle: String,
}
#[derive(Deserialize)]
pub struct Start {
    #[serde(default)]
    username: String,
    #[serde(default)]
    invitation: String,
}
async fn save_ceremony(
    state: &AppState,
    account: i64,
    kind: &str,
    ceremony: Value,
    invite: Option<Vec<u8>>,
    challenge: Value,
) -> Result<Response, AppError> {
    let mut tx = state.auth.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(account)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.ceremonies WHERE expires_at<=clock_timestamp()")
        .execute(&mut *tx)
        .await?;
    let value = auth::token();
    // Serialize the capacity check and insertion for this account.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM staff_identity.ceremonies WHERE account_id=$1")
            .bind(account)
            .fetch_one(&mut *tx)
            .await?;
    if count >= 5 {
        return Err(AppError::Invalid);
    }
    sqlx::query("INSERT INTO staff_identity.ceremonies(token_hash,account_id,kind,state,invitation_hash) VALUES ($1,$2,$3,$4::text::jsonb,$5)").bind(auth::hash(&value)).bind(account).bind(kind).bind(ceremony.to_string()).bind(invite).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut response = Json(challenge).into_response();
    set_cookie(&mut response, state, &ceremony_cookie(state), &value, 180)?;
    Ok(response)
}
pub async fn enroll_start(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if input.invitation.len() != 43 {
        return Err(AppError::Unauthorized);
    }
    let invite = auth::hash(&input.invitation);
    let account:Account=sqlx::query_as("SELECT a.id,a.username,a.user_handle FROM staff_identity.accounts a JOIN staff_identity.invitations i ON i.account_id=a.id WHERE i.token_hash=$1 AND i.expires_at>clock_timestamp() AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin')").bind(&invite).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)?;
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT credential::text FROM staff_identity.credentials WHERE account_id=$1",
    )
    .bind(account.id)
    .fetch_all(&state.auth)
    .await?;
    let exclude = existing
        .iter()
        .map(|v| serde_json::from_str::<Passkey>(v).map(|key| key.cred_id().clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Internal)?;
    let (challenge, ceremony) = state
        .webauthn
        .start_passkey_registration(
            Uuid::parse_str(&account.user_handle).map_err(|_| AppError::Internal)?,
            &account.username,
            &account.username,
            Some(exclude),
        )
        .map_err(|_| AppError::Invalid)?;
    save_ceremony(
        &state,
        account.id,
        "enroll",
        serde_json::to_value(ceremony).map_err(|_| AppError::Internal)?,
        Some(invite),
        serde_json::to_value(challenge).map_err(|_| AppError::Internal)?,
    )
    .await
}
#[derive(sqlx::FromRow)]
struct Ceremony {
    account_id: i64,
    state: String,
    invitation_hash: Option<Vec<u8>>,
}
async fn consume(state: &AppState, headers: &HeaderMap, kind: &str) -> Result<Ceremony, AppError> {
    let value = auth::cookie(headers, &ceremony_cookie(state))?;
    // Autocommit deletion precedes cryptographic verification: failed attempts
    // consume their challenge too, and cannot be replayed or retried.
    sqlx::query_as("DELETE FROM staff_identity.ceremonies WHERE token_hash=$1 AND kind=$2 AND expires_at>clock_timestamp() RETURNING account_id,state::text,invitation_hash").bind(auth::hash(&value)).bind(kind).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)
}
pub async fn enroll_finish(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<RegisterPublicKeyCredential>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let ceremony = consume(&state, &headers, "enroll").await?;
    let saved: PasskeyRegistration =
        serde_json::from_str(&ceremony.state).map_err(|_| AppError::Internal)?;
    let key = state
        .webauthn
        .finish_passkey_registration(&input, &saved)
        .map_err(|_| AppError::Unauthorized)?;
    let invite = ceremony.invitation_hash.ok_or(AppError::Unauthorized)?;
    let key_id: &[u8] = key.cred_id().as_ref();
    let account: i64 = sqlx::query_scalar("SELECT staff_identity.enroll($1,$2,$3::text::jsonb)")
        .bind(invite)
        .bind(key_id)
        .bind(serde_json::to_string(&key).map_err(|_| AppError::Internal)?)
        .fetch_one(&state.auth)
        .await?;
    if account != ceremony.account_id {
        return Err(AppError::Internal);
    }
    let mut response = Json(json!({"ok":true})).into_response();
    set_cookie(&mut response, &state, &ceremony_cookie(&state), "", 0)?;
    Ok(response)
}
pub async fn login_start(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if input.username.is_empty() || input.username.len() > 64 {
        return Err(AppError::Unauthorized);
    }
    let account:Account=sqlx::query_as("SELECT id,username,user_handle FROM staff_identity.accounts WHERE username=$1 AND revoked_at IS NULL AND role IN ('janitor','moderator','manager','admin')").bind(input.username).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)?;
    let credentials: Vec<String> = sqlx::query_scalar(
        "SELECT credential::text FROM staff_identity.credentials WHERE account_id=$1",
    )
    .bind(account.id)
    .fetch_all(&state.auth)
    .await?;
    let keys = credentials
        .iter()
        .map(|v| serde_json::from_str::<Passkey>(v))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Internal)?;
    let (challenge, ceremony) = state
        .webauthn
        .start_passkey_authentication(&keys)
        .map_err(|_| AppError::Unauthorized)?;
    save_ceremony(
        &state,
        account.id,
        "login",
        serde_json::to_value(ceremony).map_err(|_| AppError::Internal)?,
        None,
        serde_json::to_value(challenge).map_err(|_| AppError::Internal)?,
    )
    .await
}
pub async fn login_finish(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<PublicKeyCredential>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let ceremony = consume(&state, &headers, "login").await?;
    let saved: PasskeyAuthentication =
        serde_json::from_str(&ceremony.state).map_err(|_| AppError::Internal)?;
    let result = state
        .webauthn
        .finish_passkey_authentication(&input, &saved)
        .map_err(|_| AppError::Unauthorized)?;
    if !result.user_verified() {
        return Err(AppError::Unauthorized);
    }
    let key_id: &[u8] = result.cred_id().as_ref();
    let mut tx = state.auth.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ceremony.account_id)
        .execute(&mut *tx)
        .await?;
    let previous:String=sqlx::query_scalar("SELECT c.credential::text FROM staff_identity.credentials c JOIN staff_identity.accounts a ON a.id=c.account_id WHERE c.id=$1 AND a.id=$2 AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin')").bind(key_id).bind(ceremony.account_id).fetch_optional(&mut *tx).await?.ok_or(AppError::Unauthorized)?;
    let json: Value = serde_json::from_str(&previous).map_err(|_| AppError::Internal)?;
    let old_counter = json["cred"]["counter"].as_u64().ok_or(AppError::Internal)?;
    if (old_counter > 0 || result.counter() > 0) && u64::from(result.counter()) <= old_counter {
        return Err(AppError::Unauthorized);
    }
    let mut key: Passkey = serde_json::from_str(&previous).map_err(|_| AppError::Internal)?;
    key.update_credential(&result)
        .ok_or(AppError::Unauthorized)?;
    let updated = serde_json::to_string(&key).map_err(|_| AppError::Internal)?;
    let changed: bool = sqlx::query_scalar(
        "SELECT staff_identity.update_counter($1,$2::text::jsonb,$3::text::jsonb)",
    )
    .bind(key_id)
    .bind(previous)
    .bind(updated)
    .fetch_one(&mut *tx)
    .await?;
    if !changed {
        return Err(AppError::Unauthorized);
    }
    if let Ok(old) = auth::cookie(&headers, state.config.cookie_name()) {
        sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash=$1")
            .bind(auth::hash(&old))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM staff_identity.sessions WHERE expires_at<=clock_timestamp() OR last_activity_at<=clock_timestamp()-($1::bigint*interval '1 second')")
        .bind(state.config.idle_timeout.as_secs() as i64)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash IN (SELECT token_hash FROM staff_identity.sessions WHERE account_id=$1 ORDER BY authenticated_at DESC,token_hash OFFSET 9)").bind(ceremony.account_id).execute(&mut *tx).await?;
    let session = auth::token();
    let csrf = auth::token();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES ($1,$2,$3,$4)").bind(auth::hash(&session)).bind(auth::hash(&csrf)).bind(ceremony.account_id).bind(key_id).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut response = Json(json!({"ok":true})).into_response();
    set_cookie(
        &mut response,
        &state,
        state.config.cookie_name(),
        &session,
        28800,
    )?;
    set_cookie(&mut response, &state, &csrf_cookie(&state), &csrf, 28800)?;
    set_cookie(&mut response, &state, &ceremony_cookie(&state), "", 0)?;
    Ok(response)
}
pub async fn queue(State(state): Shared, headers: HeaderMap) -> Result<Html<String>, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    let session = &authority.session;
    let csrf = auth::cookie(&headers, &csrf_cookie(&state))?;
    auth::csrf(session, &csrf)?;
    let reports = store::reports(&state.staff, session)
        .await?
        .into_iter()
        .map(views::Preview::from)
        .collect();
    let html = views::Queue {
        media_origin: state.config.media_origin.clone(),
        reports,
        csrf,
        recent: session.recent,
        admin: session.role == "admin",
        moderator: session.at_least(crate::access::Level::Moderator),
        discussion: session.permissions.can_discuss(&session.role),
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingQuery {
    #[serde(default)]
    pub board: String,
    #[serde(default)]
    pub thread: i64,
    pub posted: Option<i64>,
}

pub async fn posting(
    State(state): Shared,
    headers: HeaderMap,
    Query(query): Query<PostingQuery>,
) -> Result<Html<String>, AppError> {
    let session = auth::session(&state, &headers).await?;
    if !session.at_least(crate::access::Level::Janitor)
        || query.board == "j"
        || (!query.board.is_empty() && !session.permissions.allows(&query.board))
    {
        return Err(AppError::Forbidden);
    }
    let csrf = auth::cookie(&headers, &csrf_cookie(&state))?;
    auth::csrf(&session, &csrf)?;
    if query.thread < 0
        || query.posted.is_some_and(|id| id <= 0)
        || (!query.board.is_empty() && board_domain::BoardSlug::parse(&query.board).is_err())
    {
        return Err(AppError::Invalid);
    }
    let boards: Vec<(String, String, i32)> =
        sqlx::query_as("SELECT slug,title,CASE WHEN $3 THEN max_authorized_comment_chars ELSE max_comment_chars END FROM content.boards WHERE NOT staff_only AND ('all'=ANY($1) OR slug=ANY($1)) AND NOT slug=ANY($2) ORDER BY slug LIMIT 1000")
            .bind(&session.permissions.allow_boards).bind(&session.permissions.deny_boards)
            .bind(session.at_least(crate::access::Level::Moderator))
            .fetch_all(&state.staff)
            .await?;
    if let Some(id) = query.posted {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=$1 AND p.thread_id=$2 AND p.id=$3 AND NOT p.deleted AND EXISTS(SELECT 1 FROM content.moderation_audit a WHERE a.board=p.board AND a.target_id=p.id AND a.account_id=$4 AND a.action='staff-post'))")
            .bind(&query.board).bind(query.thread).bind(id).bind(session.account_id).fetch_one(&state.staff).await?;
        if !exists {
            return Err(AppError::NotFound);
        }
    }
    let label: String = sqlx::query_scalar("SELECT coalesce(public_capcode,CASE role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) FROM staff_identity.accounts WHERE id=$1")
        .bind(session.account_id).fetch_one(&state.auth).await?;
    let level = crate::access::Level::parse(&session.role).ok_or(AppError::Unauthorized)?;
    use board_domain::capcode::Capcode;
    let eligible: Vec<_> = [
        Capcode::Moderator,
        Capcode::Administrator,
        Capcode::HighlightedAdministrator,
        Capcode::Manager,
        Capcode::Developer,
        Capcode::Founder,
    ]
    .into_iter()
    .filter(|badge| {
        level.public_capcode(badge.source_option(), &session.permissions) == Ok(Some(*badge))
    })
    .collect();
    let ordinary_ready = state.config.poster_id_key.is_some()
        && (!state.config.production || state.config.proxy.is_some());
    let selected_badge = Capcode::parse(&label)
        .filter(|badge| eligible.contains(badge))
        .or_else(|| eligible.first().copied())
        .map(|badge| badge.as_str().to_owned())
        .or_else(|| ordinary_ready.then(|| "none".to_owned()))
        .ok_or(AppError::Forbidden)?;
    let badges = eligible
        .into_iter()
        .map(|badge| {
            (
                badge.as_str().to_owned(),
                if badge.highlighted() {
                    "Admin (highlighted)"
                } else {
                    badge.label()
                }
                .to_owned(),
            )
        })
        .collect();
    let comment_max_units = boards
        .iter()
        .find(|board| board.0 == query.board)
        .or_else(|| boards.first())
        .map_or(20_000, |board| board.2 as usize * 2);
    let flags:Vec<String>=sqlx::query_scalar("SELECT DISTINCT unnest(board_flags) FROM content.boards WHERE NOT staff_only AND ('all'=ANY($1) OR slug=ANY($1)) AND NOT slug=ANY($2) ORDER BY 1 LIMIT 128")
        .bind(&session.permissions.allow_boards).bind(&session.permissions.deny_boards).fetch_all(&state.staff).await?;
    let flags = flags
        .into_iter()
        .filter_map(|code| {
            board_domain::country::board_flag(&code).map(|name| (code, name.to_owned()))
        })
        .collect();
    Ok(Html(
        views::Posting {
            public_origin: state.config.public_origin.clone(),
            boards,
            comment_max_units,
            query,
            csrf,
            recent: session.recent,
            admin: level == crate::access::Level::Admin && selected_badge == "admin",
            badges,
            selected_badge,
            ordinary_ready,
            flags,
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffMessage {
    pub csrf: String,
    pub board: String,
    #[serde(default)]
    pub thread: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub sage: bool,
    #[serde(default)]
    pub highlight: bool,
    #[serde(default)]
    pub badge: String,
    #[serde(default)]
    pub options: String,
    #[serde(default)]
    pub flag: String,
    #[serde(default)]
    pub password: String,
}

pub async fn post_message(
    State(state): Shared,
    headers: HeaderMap,
    Extension(start): Extension<StaffRequestStart>,
    Extension(peer): Extension<StaffRequestPeer>,
    Form(input): Form<StaffMessage>,
) -> Result<Redirect, AppError> {
    let request_start = start.0;
    auth::origin(&headers, &state.config.origin)?;
    let session = auth::session(&state, &headers).await?;
    if !session.at_least(crate::access::Level::Janitor)
        || input.board == "j"
        || !session.permissions.allows(&input.board)
    {
        return Err(AppError::Forbidden);
    }
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    if input.thread < 0 || board_domain::BoardSlug::parse(&input.board).is_err() {
        return Err(AppError::Invalid);
    }
    use board_domain::capcode::Capcode;
    let level = crate::access::Level::parse(&session.role).ok_or(AppError::Unauthorized)?;
    let default_badge;
    let selected = if input.badge.is_empty() {
        default_badge = sqlx::query_scalar::<_,String>("SELECT coalesce(public_capcode,CASE role WHEN 'admin' THEN 'admin' WHEN 'manager' THEN 'manager' ELSE 'mod' END) FROM staff_identity.accounts WHERE id=$1")
            .bind(session.account_id).fetch_one(&state.auth).await?;
        &default_badge
    } else {
        &input.badge
    };
    if level == crate::access::Level::Janitor && selected != "none" {
        return Err(AppError::Forbidden);
    }
    let mut raw_options = input.options.clone();
    if input.sage {
        raw_options.push_str("sage");
    }
    let source = level
        .posting_options(&raw_options, &session.permissions)
        .map_err(|error| AppError::Posting(error.0.into()))?;
    let mut badge = if selected == "none" {
        source.capcode
    } else {
        if !input.options.is_empty() || !input.flag.is_empty() || !input.password.is_empty() {
            return Err(AppError::Invalid);
        }
        Some(Capcode::parse(selected).ok_or(AppError::Invalid)?)
    };
    if input.highlight {
        if level != crate::access::Level::Admin
            || !matches!(
                badge,
                Some(Capcode::Administrator | Capcode::HighlightedAdministrator)
            )
        {
            return Err(AppError::Unauthorized);
        }
        badge = Some(Capcode::HighlightedAdministrator);
    }
    if let Some(badge) = badge
        && level.public_capcode(badge.source_option(), &session.permissions) != Ok(Some(badge))
    {
        return Err(AppError::Unauthorized);
    }
    let token = auth::cookie(&headers, state.config.cookie_name())?;
    let session_hash = auth::hash(&token);
    let csrf_hash = auth::hash(&input.csrf);
    let ticket_hash: [u8; 32] = auth::hash(&auth::token())
        .try_into()
        .map_err(|_| AppError::Internal)?;
    let authority = board_store::StaffPostAuthority {
        auth_pool: &state.auth,
        session_hash: &session_hash,
        csrf_hash: &csrf_hash,
        ticket_hash: &ticket_hash,
        idle_seconds: state.config.idle_timeout.as_secs() as i32,
        highlight: false,
        authorized_limits: level >= crate::access::Level::Moderator,
        identity: Some(board_store::StaffPostIdentity {
            capcode: badge,
            name_allowed: if selected == "none" {
                source.name_allowed
            } else {
                level.allows_capcode_name(&session.permissions)
            },
            administrator: level == crate::access::Level::Admin,
            tripcode_key: state.config.tripcode_key.as_deref(),
        }),
    };
    let mut post = board_store::NewPost {
        name: input.name,
        subject: input.subject,
        comment: input.comment,
        deletion_hash: String::new(),
        sage: source.sage,
    };
    let result = if badge.is_none() {
        if (state.config.production && state.config.proxy.is_none()) || peer.ip().is_none() {
            return Err(AppError::Internal);
        }
        let key = state
            .config
            .poster_id_key
            .as_deref()
            .ok_or(AppError::Internal)?;
        let op_hash = if input.thread > 0 {
            board_store::staff_op_deletion_hash(&state.staff, &input.board, input.thread)
                .await
                .map_err(|_| AppError::Internal)?
        } else {
            None
        };
        let (hash, proof) = crate::posting_password::prepare(input.password, op_hash).await?;
        post.deletion_hash = hash;
        board_store::create_ordinary_staff_post(
            &state.staff,
            &input.board,
            input.thread,
            &post,
            board_store::PostingContext {
                request_start,
                peer: peer.ip(),
                op_password_proof: proof,
            },
            board_store::PostMetadata {
                keys: board_store::PostIdentityKeys {
                    tripcode: state.config.tripcode_key.as_deref(),
                    poster_id: Some(key),
                },
                country_database: state.config.country_database.as_deref(),
                flag: &input.flag,
                options: &raw_options,
            },
            authority,
        )
        .await
    } else {
        board_store::create_staff_post(
            &state.staff,
            &input.board,
            input.thread,
            &post,
            request_start,
            authority,
        )
        .await
    };
    let result = match result {
        Err(board_store::StoreError::ContentQuiet { post }) => {
            return Ok(Redirect::to(&if input.thread > 0 {
                format!(
                    "{}/{}/thread/{}#p{post}",
                    state.config.public_origin, input.board, input.thread
                )
            } else {
                format!("{}/{}/", state.config.public_origin, input.board)
            }));
        }
        other => other,
    };
    let id = result.map_err(|error| match error {
        board_store::StoreError::AuthorizationChanged => AppError::Unauthorized,
        board_store::StoreError::Invalid(message) | board_store::StoreError::Conflict(message) => {
            AppError::Posting(message.into())
        }
        board_store::StoreError::NotFound => AppError::NotFound,
        board_store::StoreError::ContentRejected(message)
        | board_store::StoreError::Robot9000Rejected(message) => AppError::Posting(message),
        board_store::StoreError::Database(error) => AppError::Database(error),
        _ => AppError::Internal,
    })?;
    let thread = if input.thread == 0 { id } else { input.thread };
    if source.return_to_board {
        return Ok(Redirect::to(&format!(
            "{}/{}/",
            state.config.public_origin, input.board
        )));
    }
    Ok(Redirect::to(&format!(
        "/post?board={}&thread={thread}&posted={id}",
        input.board
    )))
}
#[derive(Deserialize)]
pub struct Mutation {
    pub csrf: String,
    #[serde(default)]
    pub board: String,
    #[serde(default)]
    pub target: i64,
    #[serde(default)]
    pub action: String,
}
pub async fn moderate(
    State(state): Shared,
    headers: HeaderMap,
    Form(input): Form<Mutation>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let mut authority = auth::guard(&state, &headers).await?;
    let session = &authority.session;
    auth::csrf(session, &input.csrf)?;
    let transaction = store::prepare_moderation(
        &state.staff,
        session,
        &input.board,
        input.target,
        &input.action,
    )
    .await?;
    authority.ensure_current(true).await?;
    transaction.commit().await?;
    authority.finish().await?;
    Ok(Redirect::to("/reports"))
}
pub async fn logout(
    State(state): Shared,
    headers: HeaderMap,
    Form(input): Form<Mutation>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let session = auth::session(&state, &headers).await?;
    auth::csrf(&session, &input.csrf)?;
    let token = auth::cookie(&headers, state.config.cookie_name())?;
    sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash=$1")
        .bind(auth::hash(&token))
        .execute(&state.auth)
        .await?;
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &state, state.config.cookie_name(), "", 0)?;
    set_cookie(&mut response, &state, &csrf_cookie(&state), "", 0)?;
    Ok(response)
}
