use crate::{AppState, handlers::AppError};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use board_domain::anonymous_session::{COOKIE_SECONDS, Capability};
use board_store::anonymous_session::PostingSession;
use chrono::{Timelike, Utc};
use std::net::IpAddr;

fn cookie_name(production: bool) -> &'static str {
    if production {
        "__Host-board-anon"
    } else {
        "board-anon"
    }
}

fn cookie(headers: &HeaderMap, production: bool) -> Result<Option<Capability>, AppError> {
    let mut size = 0;
    let mut value = None;
    let mut present = false;
    for part in headers.get_all(header::COOKIE) {
        size += part.as_bytes().len();
        if size > 8192 {
            return Err(AppError(StatusCode::BAD_REQUEST, "Invalid cookie header."));
        }
        let part = part
            .to_str()
            .map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid cookie header."))?;
        for pair in part.split(';') {
            let Some((name, token)) = pair.trim().split_once('=') else {
                continue;
            };
            if name != cookie_name(production) {
                continue;
            }
            if present {
                return Err(AppError(
                    StatusCode::BAD_REQUEST,
                    "Ambiguous anonymous session cookie.",
                ));
            }
            present = true;
            value = Capability::parse(token);
        }
    }
    Ok(value)
}

pub(crate) struct Session {
    capability: Capability,
    pub posting: PostingSession,
}

impl Session {
    /// Unknown or malformed tokens are replaced with server randomness. Client
    /// chosen values are never adopted as newly minted session capabilities.
    pub(crate) async fn resolve(
        state: &AppState,
        headers: &HeaderMap,
        peer: Option<IpAddr>,
    ) -> Result<Self, AppError> {
        let existing = Self::existing(state, headers).await?;
        let minted = existing.is_none();
        let capability = match existing {
            Some(capability) => capability,
            None => Capability::generate().map_err(|_| {
                AppError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Anonymous session is unavailable. Try again.",
                )
            })?,
        };
        let country = match (state.country_database.as_ref(), peer) {
            (Some(database), Some(peer)) => database
                .lookup(peer)
                .map_err(|_| {
                    AppError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Country lookup is unavailable.",
                    )
                })?
                .code
                .into_bytes()
                .try_into()
                .map_err(|_| {
                    AppError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Country lookup is unavailable.",
                    )
                })?,
            _ => *b"XX",
        };
        let fingerprints = capability.fingerprints(peer, country);
        let now = Utc::now().with_nanosecond(0).ok_or(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Anonymous session is unavailable. Try again.",
        ))?;
        Ok(Self {
            capability,
            posting: PostingSession {
                fingerprints,
                minted,
                now,
            },
        })
    }

    pub(crate) async fn existing(
        state: &AppState,
        headers: &HeaderMap,
    ) -> Result<Option<Capability>, AppError> {
        let Some(capability) = cookie(headers, state.production)? else {
            return Ok(None);
        };
        let present =
            board_store::anonymous_session::snapshot(&state.pool, &capability.storage_hash())
                .await?
                .is_some();
        Ok(present.then_some(capability))
    }

    pub(crate) fn password(&self, legacy: &str) -> String {
        // Explicit recovery passwords remain accepted by the earlier rewrite
        // API. Normal source forms carry an empty hidden pwd. Session ownership
        // authorizes both kinds, and an established cookie takes precedence.
        if self.posting.minted && !legacy.is_empty() {
            legacy.to_owned()
        } else {
            self.capability.credential()
        }
    }

    pub(crate) fn append(&self, headers: &mut HeaderMap, production: bool) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        let secure = if production { "; Secure" } else { "" };
        let cookie = format!(
            "{}={}; Path=/; Max-Age={COOKIE_SECONDS}; HttpOnly; SameSite=Strict{secure}",
            cookie_name(production),
            self.capability.credential(),
        );
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("canonical anonymous cookie"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> String {
        format!("a1_{}", "ab".repeat(32))
    }

    #[test]
    fn cookie_scope_and_duplicate_headers_cannot_select_ambiguous_authority() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            format!("board-anon={}", token()).parse().unwrap(),
        );
        assert!(cookie(&headers, false).unwrap().is_some());
        assert!(cookie(&headers, true).unwrap().is_none());
        headers.append(
            header::COOKIE,
            format!("board-anon={}", token()).parse().unwrap(),
        );
        assert_eq!(
            cookie(&headers, false).err().unwrap().0,
            StatusCode::BAD_REQUEST
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("__Host-board-anon={}", token()).parse().unwrap(),
        );
        assert!(cookie(&headers, true).unwrap().is_some());
        assert!(cookie(&headers, false).unwrap().is_none());
    }

    #[test]
    fn malformed_and_oversized_cookies_never_become_capabilities() {
        for value in [
            "board-anon=legacy".to_owned(),
            "board-anon=".to_owned(),
            format!("other={}", token()),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, value.parse().unwrap());
            assert!(cookie(&headers, false).unwrap().is_none());
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("other={}", "x".repeat(8192)).parse().unwrap(),
        );
        assert_eq!(
            cookie(&headers, false).err().unwrap().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn production_cookie_is_private_host_scoped_and_carries_no_public_identity() {
        let capability = Capability::parse(&token()).unwrap();
        let session = Session {
            posting: PostingSession {
                fingerprints: capability.fingerprints(None, *b"XX"),
                minted: true,
                now: Utc::now(),
            },
            capability,
        };
        let mut headers = HeaderMap::new();
        session.append(&mut headers, true);
        let value = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(value.starts_with("__Host-board-anon=a1_"));
        assert!(value.ends_with("Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict; Secure"));
        assert!(
            !value.contains("Domain=")
                && !value.contains("name=")
                && !value.contains("board-posted")
        );
    }
}
