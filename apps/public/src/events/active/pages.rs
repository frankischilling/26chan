//! Native, script-independent pages for explicitly enabled local event runs.
use super::*;
use askama::Template;
use board_domain::source_events::{STOCKS, emotes as roster};

#[derive(Template)]
#[template(path = "event-page.html")]
struct Page {
    market: bool,
    active: bool,
    has_account: bool,
    balance: i64,
    available: i64,
    ends: String,
    error: String,
    stocks: Vec<StockRow>,
    emotes: Vec<Choice>,
    obtained: Vec<String>,
}
struct StockRow {
    symbol: &'static str,
    price: Option<i64>,
    owned: i64,
}
struct Choice {
    name: &'static str,
    glyph: String,
    price: i64,
    owned: bool,
    id: &'static str,
}
impl Page {
    fn heading(&self) -> &'static str {
        if self.market {
            "Event market"
        } else {
            "Event emotes"
        }
    }
    fn path(&self) -> &'static str {
        path(if self.market {
            Kind::Market
        } else {
            Kind::Emotes
        })
    }
}
fn path(kind: Kind) -> &'static str {
    match kind {
        Kind::Market => "/event-market.html",
        Kind::Emotes => "/event-emotes.html",
    }
}
fn glyph(emote: &roster::Emote) -> String {
    if emote.kind() != roster::Kind::Unicode {
        return String::new();
    }
    emote
        .value()
        .split(';')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            part.strip_prefix("&#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32)
        })
        .collect()
}
pub(super) async fn market_page(State(state): State<AppState>, request: Request) -> Response {
    page(state, request, Kind::Market).await
}
pub(super) async fn emote_page(State(state): State<AppState>, request: Request) -> Response {
    page(state, request, Kind::Emotes).await
}
async fn page(state: AppState, request: Request, kind: Kind) -> Response {
    let headers = request.headers().clone();
    let query = request.uri().query().unwrap_or("").to_owned();
    if query.len() > 1024 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut error = String::new();
    let mut status = StatusCode::OK;
    if request.method() == Method::POST {
        let response = match kind {
            Kind::Market => super::market(State(state.clone()), request).await,
            Kind::Emotes => super::emotes(State(state.clone()), request).await,
        };
        status = response.status();
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .cloned()
            .collect();
        let body = match axum::body::to_bytes(response.into_body(), 32768).await {
            Ok(body) => body,
            Err(_) => return unavailable(&state, kind),
        };
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let success = status.is_success()
            && match kind {
                Kind::Market => value.get("balance").is_some() && value.get("error").is_none(),
                Kind::Emotes => value["status"] == "success",
            };
        if success {
            let mut destination = path(kind).to_owned();
            if let Some(items) = value["data"]["obtained"].as_array() {
                let tuples: Vec<&Value> = if items.first().is_some_and(Value::is_string) {
                    vec![&value["data"]["obtained"]]
                } else {
                    items.iter().take(10).collect()
                };
                let ids: Vec<_> = tuples
                    .into_iter()
                    .filter_map(|item| {
                        let name = item[0].as_str()?;
                        roster::roster()
                            .iter()
                            .find(|e| e.name() == name)
                            .map(roster::Emote::id)
                    })
                    .collect();
                if !ids.is_empty() {
                    destination.push('?');
                    destination.push_str(
                        &url::form_urlencoded::Serializer::new(String::new())
                            .append_pair("obtained", &ids.join(","))
                            .finish(),
                    );
                }
            }
            let mut response = (
                StatusCode::SEE_OTHER,
                [
                    (header::LOCATION, destination),
                    (header::CACHE_CONTROL, "no-store".into()),
                ],
            )
                .into_response();
            for cookie in cookies {
                response.headers_mut().append(header::SET_COOKIE, cookie);
            }
            return response;
        }
        error = value
            .get("error")
            .or_else(|| value.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("Event request unavailable")
            .to_owned();
    }
    match render(&state, kind, &headers, &query, error).await {
        Ok(mut response) => {
            *response.status_mut() = status;
            response
        }
        Err(error) => error.into_response(),
    }
}
async fn render(
    state: &AppState,
    kind: Kind,
    headers: &HeaderMap,
    query: &str,
    error: String,
) -> Result<Response, AppError> {
    let mut page = Page {
        market: kind == Kind::Market,
        active: false,
        has_account: false,
        balance: 0,
        available: 0,
        ends: String::new(),
        error,
        stocks: Vec::new(),
        emotes: Vec::new(),
        obtained: Vec::new(),
    };
    let now = chrono::Utc::now().timestamp();
    if let Some(run) = events::active_run(&state.pool, kind).await? {
        page.active = true;
        page.ends = chrono::DateTime::from_timestamp(run.ends_at(), 0)
            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();
        let session = cookie(headers, state.production, &key(state)?, run, now)?;
        let account = if let Some(session) = session {
            events::account(&state.pool, run, &session).await?
        } else {
            None
        };
        page.has_account = account.is_some();
        let account = account.unwrap_or(Value::Null);
        page.balance = account["balance"].as_i64().unwrap_or(0);
        if kind == Kind::Market {
            let quotes = events::market_quotes(&state.pool, run)
                .await?
                .unwrap_or(Value::Null);
            page.stocks = STOCKS
                .iter()
                .map(|symbol| StockRow {
                    symbol,
                    price: quotes[*symbol].as_i64(),
                    owned: account[*symbol].as_i64().unwrap_or(0),
                })
                .collect();
        } else {
            let owned: Vec<String> = account["owned"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item[0].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            if page.has_account {
                page.available = roster::Account::from_stored(
                    account["start_ts"].as_i64().unwrap_or(now),
                    page.balance,
                    &owned,
                )
                .and_then(|account| account.full_balance(now))
                .map_err(|_| {
                    AppError(StatusCode::SERVICE_UNAVAILABLE, "Event account unavailable")
                })?;
            }
            page.emotes = roster::roster()
                .iter()
                .map(|emote| Choice {
                    name: emote.name(),
                    glyph: glyph(emote),
                    price: emote.kind().price(),
                    owned: owned.iter().any(|name| name == emote.name()),
                    id: emote.id(),
                })
                .collect();
            for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
                if key != "obtained" || !page.obtained.is_empty() {
                    continue;
                }
                for id in value.split(',').take(10) {
                    if let Some(emote) =
                        roster::lookup(id).filter(|e| owned.iter().any(|name| name == e.name()))
                    {
                        page.obtained.push(emote.name().to_owned());
                    }
                }
            }
        }
    }
    let mut response = crate::output::html(state, &page)?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_glyphs_only_decode_release_metadata() {
        assert_eq!(glyph(roster::lookup("happy").unwrap()), "😀");
        assert_eq!(glyph(roster::lookup("frown").unwrap()), "☹️");
        assert_eq!(glyph(roster::lookup("03d18964").unwrap()), "");
    }
}
