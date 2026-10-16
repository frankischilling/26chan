//! Mobile automatic range events are separate from operator range/ASN rules.
//! Prefix identities are server-keyed comparison tokens, never public claims.
use super::ProviderValue;
use crate::{BoardSlug, anonymous_session::State, content_admission::AdmissionError};

pub mod creation;

pub const MAX_EVENTS: usize = 128;
pub const ACTIVE_SECONDS: u64 = 7_200;

pub struct Event {
    pub id: i64,
    pub board: String,
    /// The submitted parent: zero for an OP, thread ID for a reply.
    pub parent: i64,
    pub prefix: [u8; 32],
    pub browser: String,
    pub created_at: u64,
}

pub struct Request<'a> {
    pub board: &'a str,
    pub parent: Option<i64>,
    pub prefix: ProviderValue<Option<&'a [u8; 32]>>,
    pub browser: ProviderValue<Option<&'a str>>,
    pub request_at: u64,
    pub activity_at: u64,
    /// Database observation time, separate from the request/session clocks.
    pub event_at: u64,
    pub actor: Option<&'a State>,
}

pub struct Policy {
    events: Vec<Event>,
}

fn browser_valid(value: &str) -> bool {
    value.len() == 9
        && matches!(value.as_bytes()[0], b'0' | b'1')
        && value.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

impl Policy {
    pub fn compile(events: Vec<Event>) -> Result<Self, AdmissionError> {
        let mut ids = std::collections::HashSet::new();
        if events.len() > MAX_EVENTS
            || events.iter().any(|event| {
                event.id < 1
                    || !ids.insert(event.id)
                    || BoardSlug::parse(&event.board).is_err()
                    || event.parent < 0
                    || !browser_valid(&event.browser)
                    || !event.browser.starts_with('1')
            })
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        Ok(Self { events })
    }

    /// Called only after configured network rules have accepted the post.
    /// Trusted Pass/local-auth bypass must be resolved outside this evaluator.
    pub fn evaluate(&self, request: Request<'_>) -> Result<Option<&Event>, AdmissionError> {
        if BoardSlug::parse(request.board).is_err()
            || request.parent.is_some_and(|parent| parent < 0)
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        let Some(parent) = request.parent else {
            return Ok(None);
        };
        let candidates: Vec<_> = self
            .events
            .iter()
            .filter(|event| {
                event.board == request.board
                    && event.parent == parent
                    && match request.prefix {
                        ProviderValue::Available(Some(prefix)) => &event.prefix == prefix,
                        ProviderValue::Available(None) => false,
                        ProviderValue::Unavailable => true,
                    }
                    && u128::from(event.created_at) + u128::from(ACTIVE_SECONDS)
                        > u128::from(request.event_at)
            })
            .collect();
        if candidates.is_empty() {
            return Ok(None);
        }
        let browser = match request.browser {
            ProviderValue::Available(Some(browser)) if browser_valid(browser) => browser,
            ProviderValue::Available(Some(_)) => return Err(AdmissionError::InvalidPolicy),
            ProviderValue::Available(None) => return Ok(None),
            ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
        };
        if !browser.starts_with('1') {
            return Ok(None);
        }
        if request
            .actor
            .is_some_and(|actor| actor.is_known_or_verified(request.activity_at, 120, 1))
        {
            return Ok(None);
        }
        let prefix = match request.prefix {
            ProviderValue::Available(Some(prefix)) => prefix,
            ProviderValue::Available(None) => return Ok(None),
            ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
        };
        let since = request.actor.and_then(|actor| {
            request
                .request_at
                .checked_sub(900)
                .filter(|cutoff| actor.post_count() > 0 && actor.network_at <= *cutoff)
                .map(|_| actor.network_at)
        });
        Ok(candidates.into_iter().find(|event| {
            &event.prefix == prefix
                && event.browser == browser
                && since.is_none_or(|since| event.created_at <= since)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::net::Ipv4Addr;

    fn number(value: &Value, key: &str) -> u64 {
        value[key].as_u64().unwrap()
    }
    // Synthetic comparison tokens only. Production must use server-keyed tokens.
    fn prefix(ip: &str) -> [u8; 32] {
        let bytes = ip.parse::<Ipv4Addr>().unwrap().octets();
        let mut token = [0; 32];
        token[..2].copy_from_slice(&bytes[..2]);
        token
    }
    fn event() -> Event {
        Event {
            id: 1,
            board: "g".into(),
            parent: 123,
            prefix: [1; 32],
            browser: "1deadbeef".into(),
            created_at: 10_000,
        }
    }

    #[test]
    fn isolated_source_automatic_stage_matches_age_activity_and_scope_vectors() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../fixtures/automatic-network-reference.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1186);
        for (index, case) in cases.iter().enumerate() {
            let events = case["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| Event {
                    id: event["id"].as_i64().unwrap(),
                    board: event["board"].as_str().unwrap().into(),
                    parent: event["parent"].as_i64().unwrap(),
                    prefix: prefix(event["ip"].as_str().unwrap()),
                    browser: event["browser"].as_str().unwrap().into(),
                    created_at: number(event, "created_at"),
                })
                .collect();
            let actor = case["actor"].as_object().map(|_| {
                let input = &case["actor"];
                let at = number(case, "activity_at");
                let mut state = State::new(at - number(input, "password_age"));
                state.network_at = at - number(input, "network_age");
                state.posts = number(input, "posts") as u8;
                state.reports = number(input, "reports") as u8;
                state.pending = number(input, "pending") as u8;
                state.verified_level = number(input, "verified") as u8;
                state.change_score = number(input, "score") as u8;
                state
            });
            let prefix = prefix(case["ip"].as_str().unwrap());
            let policy = Policy::compile(events).unwrap();
            let result = policy
                .evaluate(Request {
                    board: "g",
                    parent: case["parent"].as_i64(),
                    prefix: ProviderValue::Available(Some(&prefix)),
                    browser: ProviderValue::Available(case["browser"].as_str()),
                    request_at: number(case, "request_at"),
                    activity_at: number(case, "activity_at"),
                    event_at: number(case, "event_at"),
                    actor: actor.as_ref(),
                })
                .unwrap();
            assert_eq!(
                result.is_some(),
                case["blocked"].as_bool().unwrap(),
                "automatic source vector {index}"
            );
        }
    }

    #[test]
    fn automatic_events_are_bounded_and_require_unambiguous_mobile_metadata() {
        assert!(
            Policy::compile(
                (0..=MAX_EVENTS)
                    .map(|id| Event {
                        id: id as i64 + 1,
                        ..event()
                    })
                    .collect()
            )
            .is_err()
        );
        assert!(Policy::compile(vec![event(), event()]).is_err());
        for browser in ["", "0deadbeef", "1DEADBEEF", "1deadbeefx"] {
            assert!(
                Policy::compile(vec![Event {
                    browser: browser.into(),
                    ..event()
                }])
                .is_err()
            );
        }
        assert!(
            Policy::compile(vec![Event {
                parent: -1,
                ..event()
            }])
            .is_err()
        );
        assert!(
            Policy::compile(vec![Event {
                board: "../g".into(),
                ..event()
            }])
            .is_err()
        );
    }

    #[test]
    fn providers_are_required_only_for_a_reached_event_scope() {
        let policy = Policy::compile(vec![event()]).unwrap();
        let scope = |board, parent, prefix, browser, event_at| {
            policy
                .evaluate(Request {
                    board,
                    parent,
                    prefix,
                    browser,
                    request_at: 10_001,
                    activity_at: 10_001,
                    event_at,
                    actor: None,
                })
                .map(|hit| hit.is_some())
        };
        assert!(matches!(
            scope(
                "g",
                Some(123),
                ProviderValue::Available(Some(&[1; 32])),
                ProviderValue::Unavailable,
                10_001
            ),
            Err(AdmissionError::Unavailable)
        ));
        assert!(matches!(
            scope(
                "g",
                Some(123),
                ProviderValue::Unavailable,
                ProviderValue::Available(Some("1deadbeef")),
                10_001
            ),
            Err(AdmissionError::Unavailable)
        ));
        assert!(
            !scope(
                "a",
                Some(123),
                ProviderValue::Unavailable,
                ProviderValue::Unavailable,
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                None,
                ProviderValue::Unavailable,
                ProviderValue::Unavailable,
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                Some(124),
                ProviderValue::Unavailable,
                ProviderValue::Unavailable,
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                Some(123),
                ProviderValue::Available(Some(&[2; 32])),
                ProviderValue::Unavailable,
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                Some(123),
                ProviderValue::Available(None),
                ProviderValue::Unavailable,
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                Some(123),
                ProviderValue::Unavailable,
                ProviderValue::Available(Some("0deadbeef")),
                10_001
            )
            .unwrap()
        );
        assert!(
            !scope(
                "g",
                Some(123),
                ProviderValue::Unavailable,
                ProviderValue::Unavailable,
                17_200
            )
            .unwrap()
        );
        assert!(
            scope(
                "g",
                Some(123),
                ProviderValue::Available(Some(&[1; 32])),
                ProviderValue::Available(Some("1deadbeef")),
                17_199
            )
            .unwrap()
        );
    }
}
