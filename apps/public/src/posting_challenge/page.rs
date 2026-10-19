//! Classic CAPTCHA is server-rendered so its form works without JavaScript.
use super::*;

#[derive(Default)]
pub struct View {
    pub enabled: bool,
    pub classic: bool,
    pub noscript_only: bool,
    pub exempt: bool,
    pub board: String,
    pub target: i64,
    pub purpose: &'static str,
    pub csrf: String,
    pub token: String,
    pub image: String,
    pub ttl_seconds: u16,
    pub message: &'static str,
}

fn valid_png(value: &str) -> bool {
    let Some(encoded) = value.strip_prefix("data:image/png;base64,") else {
        return false;
    };
    encoded.starts_with("iVBORw0KGgo")
        && encoded.len() <= 4 * 65536usize.div_ceil(3)
        && encoded.len().is_multiple_of(4)
        && encoded
            .trim_end_matches('=')
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
        && encoded.len() - encoded.trim_end_matches('=').len() <= 2
        && encoded.len() / 4 * 3 - (encoded.len() - encoded.trim_end_matches('=').len()) <= 65536
}

#[derive(Default)]
pub(crate) struct PageContext {
    headers: HeaderMap,
    enabled: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct CaptchaPage;

pub(crate) async fn prepare(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    scope: Scope,
) -> (View, PageContext) {
    let purpose = match scope.purpose {
        protocol::Purpose::Thread => "thread",
        protocol::Purpose::Reply => "reply",
        protocol::Purpose::Report => "report",
        _ => "",
    };
    let mut view = View {
        board: scope.board.clone(),
        target: scope.target,
        purpose,
        ..View::default()
    };
    match fill(state, headers, peer, &scope, &mut view).await {
        Ok(headers) => {
            let enabled = view.enabled;
            (view, PageContext { headers, enabled })
        }
        Err(_) => {
            view.enabled = true;
            view.exempt = false;
            view.csrf.clear();
            view.token.clear();
            view.image.clear();
            view.ttl_seconds = 0;
            view.message = "CAPTCHA is unavailable. Reload this page before posting.";
            (
                view,
                PageContext {
                    headers: HeaderMap::new(),
                    enabled: true,
                },
            )
        }
    }
}
async fn fill(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<IpAddr>,
    scope: &Scope,
    view: &mut View,
) -> Result<HeaderMap, AppError> {
    let policy = board_store::posting_challenge::board_policy(&state.pool, &scope.board).await?;
    view.enabled = policy.enabled;
    view.classic = !policy.twister;
    view.noscript_only = policy.noscript_only && !policy.twister;
    if scope.purpose != protocol::Purpose::Report
        && crate::pass_auth::posting_ready(state, headers, peer).await?
    {
        let session = Session::resolve(state, headers, peer).await?;
        state
            .verification
            .as_ref()
            .ok_or_else(unavailable)?
            .ensure_session(&crate::signin::trusted(&session, state, peer)?)
            .await
            .map_err(|_| unavailable())?;
        let secret = PostingSecret::read(headers, state.production)
            .map_err(|_| invalid())?
            .map(Ok)
            .unwrap_or_else(PostingSecret::generate)
            .map_err(|_| unavailable())?;
        view.enabled = true;
        view.exempt = true;
        view.csrf = secret.nonce();
        let mut cookies = HeaderMap::new();
        secret.append(&mut cookies, state.production);
        session.append(&mut cookies, state.production);
        return Ok(cookies);
    }
    if !policy.enabled {
        return Ok(HeaderMap::new());
    }
    // Twister loads on demand. Classic needs a real challenge in the HTML.
    if policy.twister && scope.purpose != protocol::Purpose::Report {
        return Ok(HeaderMap::new());
    }
    let session = Session::resolve(state, headers, peer).await?;
    if scope.purpose == protocol::Purpose::Report && !session.posting.minted {
        let (key, peer) = state
            .poster_id_key
            .as_ref()
            .zip(peer)
            .ok_or_else(unavailable)?;
        let identity = key.public_report_rate_identity(peer);
        view.exempt = board_store::report_challenge::advice(
            &state.pool,
            &scope.board,
            scope.target,
            &identity,
            session.posting,
            source_cookies_absent(headers)?,
        )
        .await?;
    }
    if policy.twister && !view.exempt {
        return Ok(HeaderMap::new());
    }
    let secret = PostingSecret::read(headers, state.production)
        .map_err(|_| invalid())?
        .map(Ok)
        .unwrap_or_else(PostingSecret::generate)
        .map_err(|_| unavailable())?;
    view.csrf = secret.nonce();
    if !view.exempt {
        let issued = state
            .verification
            .as_ref()
            .ok_or_else(unavailable)?
            .posting_challenge(&protocol::PostingChallengeRequest {
                context: scope.context(state, &session, peer, &view.csrf)?,
                ticket: None,
                ticket_answer: None,
            })
            .await
            .map_err(|_| unavailable())?;
        match issued.challenge {
            protocol::Challenge::Ready {
                token,
                image,
                background: None,
                ttl_seconds,
                slider: false,
                ..
            } if !issued.ticket_challenge
                && valid_png(&image)
                && (4..=120).contains(&ttl_seconds)
                && board_domain::challenge_secrets::VerificationToken::parse(
                    board_domain::challenge_secrets::TokenKind::Challenge,
                    &token,
                )
                .is_some() =>
            {
                view.token = token;
                view.image = image;
                view.ttl_seconds = ttl_seconds;
            }
            _ => return Err(unavailable()),
        }
    }
    let mut cookies = HeaderMap::new();
    secret.append(&mut cookies, state.production);
    session.append(&mut cookies, state.production);
    Ok(cookies)
}
pub(crate) fn finish(mut response: Response, context: &PageContext) -> Response {
    if context.enabled {
        response.extensions_mut().insert(CaptchaPage);
    }
    for value in context.headers.get_all(header::SET_COOKIE) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, value.clone());
    }
    if !context.headers.is_empty() {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    }
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_classic_images_accept_only_bounded_png_data() {
        assert!(valid_png("data:image/png;base64,iVBORw0KGgo="));
        for image in [
            "https://example.test/a.png",
            "data:image/svg+xml;base64,iVBORw0KGgo=",
            "data:image/png;base64,iVBORw0KGgo====",
            "data:image/png;base64,iVBORw0KGgo\"",
            "data:image/png;base64,iVBORw0KGgo=AAAA",
        ] {
            assert!(!valid_png(image));
        }
    }
}
