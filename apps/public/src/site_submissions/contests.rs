use crate::{
    AppState, anonymous_session::Session, handlers::AppError, security::RequestPeer,
    verification_form::ContestVoteSecret,
};
use askama::Template;
use axum::{
    Extension, Form, Router,
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{Redirect, Response},
    routing::get,
};
use board_domain::{BoardSlug, challenge_secrets::ReceiptToken, site_submissions::contests::Kind};
use board_store::site_submissions::{
    ContestProfile,
    contests::{self, Directory, Listing},
    feedback::Proof,
};
use board_verification_client::protocol;
use serde::Deserialize;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/contests",
            get(|| async { Redirect::temporary("/contests/banners") }),
        )
        .route("/contests/{kind}", get(directory))
        .route(
            "/contests/{kind}/{campaign}/{board}",
            get(listing).post(vote).layer(DefaultBodyLimit::max(512)),
        )
}

#[derive(Template)]
#[template(path = "contest-directory.html")]
struct DirectoryPage {
    kind: Kind,
    directory: Directory,
}

impl DirectoryPage {
    fn kind_slug(&self) -> &'static str {
        slug(self.kind)
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Kind::BoardBanner => "Board banner contests",
            Kind::Vtuber => "VTuber banner contests",
        }
    }

    fn date(&self, timestamp: &i64) -> String {
        chrono::DateTime::from_timestamp(*timestamp, 0).map_or_else(String::new, |date| {
            date.format("%-d %b %Y, %H:%M UTC").to_string()
        })
    }
}

#[derive(Template)]
#[template(path = "contest-entries.html")]
struct EntriesPage {
    kind: Kind,
    profile: ContestProfile,
    listing: Listing,
    media_origin: Option<String>,
    voting_configured: bool,
    csrf: Option<String>,
}

impl EntriesPage {
    fn kind_slug(&self) -> &'static str {
        slug(self.kind)
    }

    fn is_selected(&self, board: &str) -> bool {
        board == self.listing.board
    }
}

#[derive(Template)]
#[template(path = "contest-vote-result.html")]
struct VoteResult {
    accepted: bool,
    return_to: String,
}

fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Contest not found.")
}

fn bad() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid contest request.")
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Contest voting is unavailable.",
    )
}

fn sign_in() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "Sign in with a current Pass to vote.",
    )
}

fn slug(kind: Kind) -> &'static str {
    match kind {
        Kind::BoardBanner => "banners",
        Kind::Vtuber => "vtubers",
    }
}

fn parse_kind(part: &str) -> Result<Kind, AppError> {
    match part {
        "banners" => Ok(Kind::BoardBanner),
        "vtubers" => Ok(Kind::Vtuber),
        _ => Err(not_found()),
    }
}

fn positive(value: &str) -> Option<i64> {
    if value.starts_with('0') || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<i64>().ok().filter(|value| *value > 0)
}

fn position(query: Option<&str>, key: &str) -> Result<i64, AppError> {
    match query {
        None => Ok(0),
        Some(query) => positive(query.strip_prefix(key).ok_or_else(bad)?).ok_or_else(bad),
    }
}

fn no_store(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
}

async fn directory(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let kind = parse_kind(&kind)?;
    let before = position(query.as_deref(), "before=")?;
    let directory = contests::directory(&state.pool, kind, before).await?;
    let mut response = crate::output::html(&state, &DirectoryPage { kind, directory })?;
    no_store(&mut response);
    Ok(response)
}

struct Selection {
    kind: Kind,
    campaign: i64,
    board: String,
    offset: i32,
}

impl Selection {
    fn new(path: (String, String, String), query: Option<&str>) -> Result<Self, AppError> {
        let (kind, campaign, board) = path;
        let kind = parse_kind(&kind)?;
        let campaign = positive(&campaign).ok_or_else(not_found)?;
        BoardSlug::parse(&board).map_err(|_| not_found())?;
        let offset = position(query, "offset=")?;
        let offset = i32::try_from(offset)
            .ok()
            .filter(|n| *n <= 100_000)
            .ok_or_else(bad)?;
        Ok(Self {
            kind,
            campaign,
            board,
            offset,
        })
    }

    fn url(&self) -> String {
        let mut path = format!(
            "/contests/{}/{}/{}",
            slug(self.kind),
            self.campaign,
            self.board
        );
        if self.offset != 0 {
            path.push_str(&format!("?offset={}", self.offset));
        }
        path
    }

    async fn read(&self, state: &AppState) -> Result<(ContestProfile, Listing), AppError> {
        let profile = contests::campaign(&state.pool, self.campaign)
            .await?
            .filter(|profile| profile.campaign.kind == self.kind)
            .ok_or_else(not_found)?;
        let listing = contests::listing(&state.pool, self.campaign, &self.board, self.offset)
            .await?
            .ok_or_else(not_found)?;
        Ok((profile, listing))
    }
}

async fn listing(
    State(state): State<AppState>,
    Path(path): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let selection = Selection::new(path, query.as_deref())?;
    let (profile, listing) = selection.read(&state).await?;
    let media_origin = state
        .media
        .as_ref()
        .map(|media| media.settings.origin.as_string());
    let mut voting_configured =
        media_origin.is_some() && state.verification.is_some() && peer.is_some();
    let mut cookies = HeaderMap::new();
    let mut csrf = None;
    if method == Method::GET
        && listing.voting
        && !listing.entries.is_empty()
        && voting_configured
        && let Some(service) = &state.verification
        && let Some(token) = crate::pass_auth::read_cookie(&headers, state.production)?
    {
        match service.pass_state(&protocol::PassSession { token }).await {
            Ok(protocol::PassState::Ready { .. }) => {
                let session = Session::resolve(&state, &headers, peer).await?;
                if session.posting.minted {
                    let registered = service
                        .ensure_session(&crate::signin::trusted(&session, &state, peer)?)
                        .await
                        .map_err(|_| unavailable())?;
                    if !registered.changed {
                        return Err(unavailable());
                    }
                    session.append(&mut cookies, state.production);
                }
                let secret = match ContestVoteSecret::read(&headers, state.production)
                    .map_err(|_| bad())?
                {
                    Some(secret) => secret,
                    None => {
                        let secret = ContestVoteSecret::generate().map_err(|_| unavailable())?;
                        secret.append(&mut cookies, state.production);
                        secret
                    }
                };
                csrf = Some(secret.nonce());
            }
            Ok(_) => {}
            Err(_) => voting_configured = false,
        }
    }
    let mut response = crate::output::html(
        &state,
        &EntriesPage {
            kind: selection.kind,
            profile,
            listing,
            media_origin,
            voting_configured,
            csrf,
        },
    )?;
    for cookie in cookies.get_all(header::SET_COOKIE) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.clone());
    }
    no_store(&mut response);
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Vote {
    entry: String,
    csrf: String,
}

async fn vote(
    State(state): State<AppState>,
    Path(path): Path<(String, String, String)>,
    RawQuery(query): RawQuery,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    Form(form): Form<Vote>,
) -> Result<Response, AppError> {
    let selection = Selection::new(path, query.as_deref())?;
    let entry = positive(&form.entry).ok_or_else(bad)?;
    let secret = ContestVoteSecret::read(&headers, state.production)
        .map_err(|_| bad())?
        .filter(|secret| secret.matches(&form.csrf))
        .ok_or(AppError(
            StatusCode::FORBIDDEN,
            "Reload the voting form and try again.",
        ))?;
    let token = crate::pass_auth::read_cookie(&headers, state.production)?.ok_or_else(sign_in)?;
    if state.media.is_none() {
        return Err(unavailable());
    }
    let verifier = state.verification.as_ref().ok_or_else(unavailable)?;
    let (_, listing) = selection.read(&state).await?;
    if !listing.voting {
        return Err(AppError(StatusCode::CONFLICT, "Contest voting is closed."));
    }
    if !listing
        .entries
        .iter()
        .any(|candidate| candidate.id == entry)
    {
        return Err(not_found());
    }
    let session = Session::resolve(&state, &headers, peer).await?;
    if session.posting.minted {
        return Err(sign_in());
    }
    let proof = verifier
        .pass_write_receipt(&protocol::PassWriteRequest {
            token,
            context: protocol::ChallengeContext {
                session: crate::signin::trusted(&session, &state, peer)?,
                purpose: protocol::Purpose::SiteContestVote,
                board: None,
                target: entry,
                form_nonce: secret.nonce(),
                email_token: None,
            },
        })
        .await
        .map_err(|_| unavailable())?;
    let (receipt, peer_hash, context_hash) = match proof {
        protocol::PassWriteResult::Verified {
            receipt,
            peer_hash,
            context_hash,
        } => (receipt, peer_hash, context_hash),
        protocol::PassWriteResult::Capacity {} => return Err(unavailable()),
        _ => return Err(sign_in()),
    };
    let receipt = ReceiptToken::parse(&receipt)
        .ok_or_else(unavailable)?
        .storage_hash();
    let accepted = contests::vote(
        &state.pool,
        selection.campaign,
        entry,
        &Proof {
            receipt: &receipt,
            peer: &peer_hash,
            session: &session.posting.fingerprints.token,
            context: &context_hash,
        },
    )
    .await?;
    let mut response = crate::output::html(
        &state,
        &VoteResult {
            accepted,
            return_to: selection.url(),
        },
    )?;
    if !accepted {
        *response.status_mut() = StatusCode::CONFLICT;
    }
    no_store(&mut response);
    Ok(response)
}
