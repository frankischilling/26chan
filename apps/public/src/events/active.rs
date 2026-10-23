//! Explicitly enabled local reconstructions of dormant source controllers.
//! Run activation and account state remain in separate private SQL authority.
mod pages;

use crate::{AppState, handlers::AppError};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, FromRequest, Multipart, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
};
use board_domain::source_events::{
    credentials::{Key, Kind, Run, Session},
    market::{Order, Side},
};
use board_store::events;
use serde::{Deserialize, de};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::IpAddr};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .merge(super::likes::routes())
        .merge(super::perks::routes())
        .route("/emotes_xa22.php", any(emotes))
        .route("/xa24tb.php", any(market))
        .route(
            "/event-market.html",
            axum::routing::get(pages::market_page).post(pages::market_page),
        )
        .route(
            "/event-emotes.html",
            axum::routing::get(pages::emote_page).post(pages::emote_page),
        )
        .layer(DefaultBodyLimit::max(8192))
}
fn reply(state: &AppState, value: Value) -> Response {
    match crate::output::json(state.limits.response_writer(32768), &value) {
        Ok(encoded) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            encoded.into_body(),
        )
            .into_response(),
        Err(error) => error.into_response(),
    }
}
fn market_error(state: &AppState, message: &str) -> Response {
    reply(state, json!({"error":message}))
}
fn emote_error(state: &AppState, message: &str) -> Response {
    reply(state, json!({"status":"error","msg":message}))
}
fn unavailable(state: &AppState, kind: Kind) -> Response {
    let mut response = match kind {
        Kind::Market => market_error(state, "Event service unavailable"),
        Kind::Emotes => emote_error(state, "Event service unavailable"),
    };
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    response
}
fn bad_form(state: &AppState, kind: Kind, error: AppError) -> Response {
    let mut response = match kind {
        Kind::Market => market_error(state, "Bad request"),
        Kind::Emotes => emote_error(state, "Bad Request"),
    };
    if response.status().is_success() {
        *response.status_mut() = error.0;
    }
    response
}
fn key(state: &AppState) -> Result<Key, AppError> {
    state
        .poster_id_key
        .as_ref()
        .map(|key| key.event_key())
        .ok_or(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Event service unavailable",
        ))
}
fn peer(request: &Request) -> Option<IpAddr> {
    request
        .extensions()
        .get::<crate::security::RequestPeer>()
        .and_then(|peer| peer.0)
        .map(|peer| peer.to_canonical())
        .filter(|peer| !peer.is_unspecified())
}
fn cookie_name(kind: Kind, production: bool) -> &'static str {
    match (kind, production) {
        (Kind::Market, false) => "board-event-market",
        (Kind::Emotes, false) => "board-event-emotes",
        (Kind::Market, true) => "__Host-board-event-market",
        (Kind::Emotes, true) => "__Host-board-event-emotes",
    }
}
fn cookie(
    headers: &HeaderMap,
    production: bool,
    key: &Key,
    run: Run,
    now: i64,
) -> Result<Option<Session>, AppError> {
    let mut length = 0usize;
    let mut value = None;
    let mut found = false;
    for header in headers.get_all(header::COOKIE) {
        length = length.saturating_add(header.as_bytes().len());
        if length > 8192 {
            return Err(AppError(StatusCode::BAD_REQUEST, "Invalid event cookie"));
        }
        let header = header
            .to_str()
            .map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid event cookie"))?;
        for pair in header.split(';') {
            let Some((name, token)) = pair.trim().split_once('=') else {
                continue;
            };
            if name != cookie_name(run.kind(), production) {
                continue;
            }
            if found {
                return Err(AppError(StatusCode::BAD_REQUEST, "Ambiguous event cookie"));
            }
            found = true;
            value = key.parse(run, token, now);
        }
    }
    Ok(value)
}
fn append_cookie(response: &mut Response, session: &Session, production: bool, now: i64) {
    let secure = if production { "; Secure" } else { "" };
    let value = format!(
        "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Strict{secure}",
        cookie_name(session.run().kind(), production),
        session.credential(),
        session.expires_at().saturating_sub(now).max(0)
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).expect("canonical event credential"),
    );
}

struct Fields(Vec<(String, String)>);
impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Fields;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("bounded event fields")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut values: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = values.next_element()? {
                    if fields.len() >= 5 {
                        return Err(de::Error::custom("Too many event fields"));
                    }
                    fields.push(field);
                }
                Ok(Fields(fields))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}
async fn fields(
    state: &AppState,
    request: Request,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, AppError> {
    if request.method() != Method::POST {
        return Ok(BTreeMap::new());
    }
    let invalid = || AppError(StatusCode::UNPROCESSABLE_ENTITY, "Invalid event form");
    let multipart = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("multipart/form-data"));
    let pairs = if multipart {
        let mut reader = Multipart::from_request(request, state)
            .await
            .map_err(|e| AppError(e.status(), "Invalid event form"))?;
        let mut pairs = Vec::new();
        while let Some(field) = reader
            .next_field()
            .await
            .map_err(|e| AppError(e.status(), "Invalid event form"))?
        {
            if pairs.len() >= 5 || field.file_name().is_some() {
                return Err(invalid());
            }
            let name = field
                .name()
                .filter(|value| value.len() <= 16)
                .ok_or_else(invalid)?
                .to_owned();
            let value = field
                .text()
                .await
                .map_err(|e| AppError(e.status(), "Invalid event form"))?;
            pairs.push((name, value));
        }
        pairs
    } else {
        Form::<Fields>::from_request(request, state)
            .await
            .map_err(|e| AppError(e.status(), "Invalid event form"))?
            .0
            .0
    };
    let mut result = BTreeMap::new();
    for (name, value) in pairs {
        if !allowed.contains(&name.as_str())
            || value.len() > 100
            || result.insert(name, value).is_some()
        {
            return Err(invalid());
        }
    }
    Ok(result)
}
async fn established(
    state: &AppState,
    headers: &HeaderMap,
    peer: IpAddr,
    now: i64,
) -> Result<bool, AppError> {
    let anonymous = crate::anonymous_session::Session::resolve(state, headers, Some(peer)).await?;
    if anonymous.posting.minted {
        return Ok(false);
    }
    let fingerprints = anonymous.posting.fingerprints;
    let snapshot =
        board_store::anonymous_session::snapshot(&state.pool, &fingerprints.token).await?;
    let now = u64::try_from(now)
        .map_err(|_| AppError(StatusCode::SERVICE_UNAVAILABLE, "Event service unavailable"))?;
    Ok(snapshot
        .is_some_and(|snapshot| snapshot.for_request(now, &fingerprints).network_age(now) >= 3600))
}

async fn market(State(state): State<AppState>, request: Request) -> Response {
    let run = match events::active_run(&state.pool, Kind::Market).await {
        Ok(Some(run)) => run,
        Ok(None) => return super::market_closed().await,
        Err(_) => return unavailable(&state, Kind::Market),
    };
    let Some(peer) = peer(&request) else {
        return unavailable(&state, Kind::Market);
    };
    let headers = request.headers().clone();
    let now = chrono::Utc::now().timestamp();
    let fields = match fields(
        &state,
        request,
        &["create", "mode", "stock", "amount", "price"],
    )
    .await
    {
        Ok(fields) => fields,
        Err(error) => return bad_form(&state, Kind::Market, error),
    };
    let key = match key(&state) {
        Ok(key) => key,
        Err(_) => return unavailable(&state, Kind::Market),
    };
    if fields.contains_key("create") {
        let session = match cookie(&headers, state.production, &key, run, now) {
            Ok(Some(session)) => session,
            Ok(None) => match key.issue_for_peer(run, peer, now) {
                Ok(session) => session,
                Err(_) => return super::market_closed().await,
            },
            Err(error) => return bad_form(&state, Kind::Market, error),
        };
        return match events::open_account(&state.pool, run, &session, false).await {
            Ok(Some(value)) => {
                let mut response = reply(&state, value);
                append_cookie(&mut response, &session, state.production, now);
                response
            }
            Ok(None) => super::market_closed().await,
            Err(_) => unavailable(&state, Kind::Market),
        };
    }
    let side = match fields.get("mode").map(String::as_str) {
        Some("buy") => Side::Buy,
        Some("sell") => Side::Sell,
        _ => return market_error(&state, "Bad request"),
    };
    let value = |name| fields.get(name).map_or("", String::as_str);
    let order = match Order::parse(side, value("stock"), value("amount"), value("price")) {
        Ok(order) => order,
        Err(error) => return market_error(&state, error.0),
    };
    // Source validates the current quote before asking for the account. SQL
    // repeats this check under locks when it performs the actual trade.
    let quotes = match events::market_quotes(&state.pool, run).await {
        Ok(Some(value)) => value,
        Ok(None) => return super::market_closed().await,
        Err(_) => return unavailable(&state, Kind::Market),
    };
    let Some(current) = quotes.get(order.stock()).and_then(Value::as_i64) else {
        return market_error(&state, "Invalid price");
    };
    if current != order.price() {
        return market_error(&state, "The price has changed.");
    }
    let session = match cookie(&headers, state.production, &key, run, now) {
        Ok(Some(session)) => session,
        Ok(None) => return market_error(&state, "Account not found"),
        Err(error) => return bad_form(&state, Kind::Market, error),
    };
    match events::market_trade(&state.pool, run, &session, order).await {
        Ok(Some(value)) => reply(&state, value),
        Ok(None) => super::market_closed().await,
        Err(_) => unavailable(&state, Kind::Market),
    }
}
async fn emotes(State(state): State<AppState>, request: Request) -> Response {
    let run = match events::active_run(&state.pool, Kind::Emotes).await {
        Ok(Some(run)) => run,
        Ok(None) => return super::emotes_closed().await,
        Err(_) => return unavailable(&state, Kind::Emotes),
    };
    let Some(peer) = peer(&request) else {
        return unavailable(&state, Kind::Emotes);
    };
    let headers = request.headers().clone();
    let now = chrono::Utc::now().timestamp();
    let fields = match fields(&state, request, &["action", "eid"]).await {
        Ok(fields) => fields,
        Err(error) => return bad_form(&state, Kind::Emotes, error),
    };
    let key = match key(&state) {
        Ok(key) => key,
        Err(_) => return unavailable(&state, Kind::Emotes),
    };
    let existing = match cookie(&headers, state.production, &key, run, now) {
        Ok(session) => session,
        Err(error) => return bad_form(&state, Kind::Emotes, error),
    };
    let action = fields.get("action").map_or("", String::as_str);
    if action == "start" {
        if let Some(session) = existing.as_ref() {
            match events::account(&state.pool, run, session).await {
                Ok(Some(value)) => return reply(&state, json!({"status":"success","data":value})),
                Ok(None) => {}
                Err(_) => return unavailable(&state, Kind::Emotes),
            }
        }
        let session = match key.issue_for_peer(run, peer, now) {
            Ok(session) => session,
            Err(_) => return super::emotes_closed().await,
        };
        let known = match events::account(&state.pool, run, &session).await {
            Ok(value) => value,
            Err(_) => return unavailable(&state, Kind::Emotes),
        };
        let result = if let Some(value) = known {
            Some(value)
        } else {
            let eligible = match established(&state, &headers, peer, now).await {
                Ok(value) => value,
                Err(_) => return unavailable(&state, Kind::Emotes),
            };
            match events::open_account(&state.pool, run, &session, eligible).await {
                Ok(value) => value,
                Err(_) => return unavailable(&state, Kind::Emotes),
            }
        };
        return match result {
            Some(value) => {
                if let Some(error) = value.get("error").and_then(Value::as_str) {
                    return emote_error(&state, error);
                }
                let mut response = reply(&state, json!({"status":"success","data":value}));
                append_cookie(&mut response, &session, state.production, now);
                response
            }
            None => super::emotes_closed().await,
        };
    }
    let Some(session) = existing else {
        return emote_error(&state, "Bad Request");
    };
    let result = match action {
        "buy" => {
            let id = fields.get("eid").map_or("", String::as_str);
            if board_domain::source_events::emotes::lookup(id).is_none() {
                return emote_error(&state, "Bad Request");
            }
            events::buy_emote(&state.pool, run, &session, id).await
        }
        "roll3" => events::roll_emotes(&state.pool, run, &session, 3).await,
        "roll10" => events::roll_emotes(&state.pool, run, &session, 10).await,
        _ => return emote_error(&state, "Bad Request"),
    };
    match result {
        Ok(Some(value)) => reply(&state, value),
        Ok(None) => super::emotes_closed().await,
        Err(_) => unavailable(&state, Kind::Emotes),
    }
}
