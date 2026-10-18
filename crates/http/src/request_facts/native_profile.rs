//! Explicit native HTTP/1 scoring profile, not a recovered historical SAPI.
//! Only parser-ordered HTTP fields occur in the server-entry list. Trusted time
//! stays separate; no invented non-HTTP entries are interleaved with headers.
use super::CapturedRequest;
use board_config::Origin;
use board_domain::content_admission::{network::ProviderValue, threat};

/// These values must come from server request time and verified-peer geography.
/// Header fields cannot select country, request time, mode or configured origin.
pub struct Context<'a> {
    pub request_time: ProviderValue<i64>,
    pub challenge: Option<&'a [u8]>,
    pub response: Option<&'a [u8]>,
    pub mode: Mode<'a>,
}

pub enum Mode<'a> {
    Posting {
        is_op: bool,
        country: ProviderValue<&'a [u8]>,
    },
    Reporting,
}

/// Caller policy must distinguish an explicitly disabled classifier from
/// unavailable evidence required by an enabled classifier. This function never
/// turns unavailable capture, cookies, time or country into a successful score.
pub fn score(
    captured: ProviderValue<&CapturedRequest>,
    origin: &Origin,
    context: Context<'_>,
) -> ProviderValue<threat::Score> {
    with_request(captured, origin, context, threat::score_for_native_origin)
}

/// Reporting-only observation; callers cannot substitute a posting score.
pub fn report_score(
    captured: ProviderValue<&CapturedRequest>,
    origin: &Origin,
    request_time: ProviderValue<i64>,
    challenge: Option<&[u8]>,
    response: Option<&[u8]>,
) -> ProviderValue<threat::NativeReportScore> {
    with_request(
        captured,
        origin,
        Context {
            request_time,
            challenge,
            response,
            mode: Mode::Reporting,
        },
        threat::score_native_report,
    )
}
fn with_request<T>(
    captured: ProviderValue<&CapturedRequest>,
    origin: &Origin,
    context: Context<'_>,
    evaluate: impl FnOnce(ProviderValue<threat::Request<'_>>, ProviderValue<bool>) -> ProviderValue<T>,
) -> ProviderValue<T> {
    let ProviderValue::Available(captured) = captured else {
        return ProviderValue::Unavailable;
    };
    let ProviderValue::Available(cookies) = captured.cookies() else {
        return ProviderValue::Unavailable;
    };
    let ProviderValue::Available(request_time) = context.request_time else {
        return ProviderValue::Unavailable;
    };
    let (is_op, multipart, country) = match context.mode {
        Mode::Posting { is_op, country } => {
            let ProviderValue::Available(country) = country else {
                return ProviderValue::Unavailable;
            };
            if country.len() != 2 || !country.iter().all(u8::is_ascii_uppercase) {
                return ProviderValue::Unavailable;
            }
            (is_op, true, Some(country))
        }
        Mode::Reporting => (true, false, None),
    };
    let server: Vec<_> = captured
        .http_fields()
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_slice()))
        .collect();
    let cookies: Vec<_> = cookies
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_slice()))
        .collect();
    let same_origin = server
        .iter()
        .find(|(key, _)| *key == "HTTP_REFERER")
        .is_some_and(|(_, value)| {
            std::str::from_utf8(value).is_ok_and(|url| origin.matches_url(url))
        });
    evaluate(
        ProviderValue::Available(threat::Request {
            server: &server,
            cookies: &cookies,
            challenge: context.challenge,
            response: context.response,
            request_time,
            country,
            is_op,
            multipart,
        }),
        ProviderValue::Available(same_origin),
    )
}

#[cfg(test)]
mod tests;
