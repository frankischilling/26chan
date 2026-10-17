//! Bounded source classifier predicates over qualified server request facts.
//! These predicates do not grant CAPTCHA, posting, or staff authority.
use super::network::ProviderValue;

const MAX_HEADER_BYTES: usize = 4096;
type Fact = ProviderValue<bool>;

#[derive(Clone, Copy)]
pub struct CreditRequest<'a> {
    pub accept_language: ProviderValue<Option<&'a [u8]>>,
    pub accept: ProviderValue<Option<&'a [u8]>>,
    pub user_agent: ProviderValue<Option<&'a [u8]>>,
    pub referer: ProviderValue<Option<&'a [u8]>>,
}

fn not(value: Fact) -> Fact {
    match value {
        ProviderValue::Available(value) => ProviderValue::Available(!value),
        ProviderValue::Unavailable => ProviderValue::Unavailable,
    }
}
fn and(left: Fact, right: Fact) -> Fact {
    match (left, right) {
        (ProviderValue::Available(false), _) | (_, ProviderValue::Available(false)) => {
            ProviderValue::Available(false)
        }
        (ProviderValue::Available(true), ProviderValue::Available(true)) => {
            ProviderValue::Available(true)
        }
        _ => ProviderValue::Unavailable,
    }
}
fn or(left: Fact, right: Fact) -> Fact {
    match (left, right) {
        (ProviderValue::Available(true), _) | (_, ProviderValue::Available(true)) => {
            ProviderValue::Available(true)
        }
        (ProviderValue::Available(false), ProviderValue::Available(false)) => {
            ProviderValue::Available(false)
        }
        _ => ProviderValue::Unavailable,
    }
}
fn present(value: ProviderValue<Option<&[u8]>>) -> Fact {
    match value {
        ProviderValue::Available(value) => ProviderValue::Available(value.is_some()),
        ProviderValue::Unavailable => ProviderValue::Unavailable,
    }
}
fn contains(value: ProviderValue<Option<&[u8]>>, needle: &[u8]) -> Fact {
    match value {
        ProviderValue::Available(None) => ProviderValue::Available(false),
        ProviderValue::Available(Some(value)) if value.len() <= MAX_HEADER_BYTES => {
            ProviderValue::Available(value.windows(needle.len()).any(|part| part == needle))
        }
        _ => ProviderValue::Unavailable,
    }
}

/// The source uses this only to withhold bypass credits after a successful
/// CAPTCHA. It is not an independent posting rejection or a failed-CAPTCHA
/// success signal. Empty headers are present, matching PHP's `isset` behavior.
/// Unknown or truncated required inputs cannot qualify a negative result.
pub fn withhold_captcha_credit(request: CreditRequest<'_>) -> Fact {
    let no_language = not(present(request.accept_language));
    let no_accept = not(present(request.accept));
    let missing_both = and(no_language, no_accept);
    let android = and(no_language, contains(request.user_agent, b"; wv)"));
    let iphone = and(
        no_accept,
        and(
            contains(request.user_agent, b"iPhone"),
            not(contains(request.user_agent, b"Mobile")),
        ),
    );
    let referer = and(
        no_language,
        or(
            contains(request.referer, b"sys.4chan.org"),
            contains(request.referer, b"/thread/"),
        ),
    );
    or(or(missing_both, android), or(iphone, referer))
}

#[derive(Clone, Copy)]
pub struct AutomationRequest<'a> {
    /// Only a qualified trusted-edge provider may supply this score. A public
    /// HTTP header is not evidence. None means the provider confirmed absence.
    pub edge_score: ProviderValue<Option<i64>>,
    pub user_agent: ProviderValue<Option<&'a [u8]>>,
    /// Qualified presence/truth of the private cached marker. A deliberately
    /// disabled cache is Available(false); lookup failure is Unavailable.
    pub cached_marker: ProviderValue<bool>,
}

/// Source default-threshold classification. The caller must separately check
/// session age, bypass authority and private cooldown history before refusing.
pub fn likely_automated(request: AutomationRequest<'_>) -> Fact {
    let (header_present, low_score) = match request.edge_score {
        ProviderValue::Available(None) => return ProviderValue::Available(false),
        ProviderValue::Available(Some(score)) => (
            ProviderValue::Available(true),
            ProviderValue::Available((1..=29).contains(&score)),
        ),
        ProviderValue::Unavailable => (ProviderValue::Unavailable, ProviderValue::Unavailable),
    };
    let ios = and(
        or(
            contains(request.user_agent, b"iPhone"),
            contains(request.user_agent, b"iPad"),
        ),
        not(or(
            contains(request.user_agent, b"Mobile"),
            contains(request.user_agent, b"Safari"),
        )),
    );
    let webview = or(contains(request.user_agent, b"; wv)"), ios);
    and(
        header_present,
        and(not(webview), or(low_score, request.cached_marker)),
    )
}

#[cfg(test)]
mod tests;
