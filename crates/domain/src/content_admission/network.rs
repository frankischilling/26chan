//! Bounded operator-owned range/ASN policy. Address and ASN values must come
//! from the server's trusted peer/resolver path, never public forwarding fields.
use super::AdmissionError;
use crate::{BoardSlug, anonymous_session::State};
use std::net::Ipv4Addr;

pub mod asn;

pub const MAX_NETWORK_RULES: usize = 128;
pub const MAX_SCOPE_BOARDS: usize = 128;
pub const MAX_SIGNATURE_LIST_BYTES: usize = 2_048;
pub const MAX_SIGNATURE_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Leniency {
    None,
    KnownOrVerified,
    Known,
    Verified,
}

pub struct Rule {
    pub id: i64,
    pub range: Option<(Ipv4Addr, Ipv4Addr)>,
    pub asn: Option<u32>,
    pub active: bool,
    /// Zero means permanent, matching the source policy's timestamp convention.
    pub expires_at: u64,
    pub created_at: u64,
    pub updated_at: u64,
    /// Comma-separated source scopes. Empty or exactly `0` is global under
    /// PHP truthiness; `_ws_` selects work-safe boards. Preserve delimiters.
    pub boards: String,
    pub ops_only: bool,
    pub images_only: bool,
    pub reports_only: bool,
    pub leniency: Leniency,
    /// The source tests literal substrings, not regex or comma-delimited tokens.
    pub signatures: String,
}

/// An unavailable provider is distinct from a successful lookup with no match.
#[derive(Clone, Copy)]
pub enum ProviderValue<T> {
    Available(T),
    Unavailable,
}

#[derive(Clone, Copy)]
pub struct Signatures<'a> {
    pub browser_id: Option<&'a str>,
    pub request_signature: Option<&'a str>,
}

pub struct Request<'a> {
    /// Rule expiry uses the request clock; activity keeps its snapshot clock.
    pub now: u64,
    pub activity_now: Option<u64>,
    pub address: Option<Ipv4Addr>,
    pub asn: ProviderValue<Option<u32>>,
    pub board: Option<&'a str>,
    pub work_safe: bool,
    pub op: bool,
    pub image: bool,
    pub report: bool,
    pub signatures: ProviderValue<Signatures<'a>>,
    /// A locked, request-adjusted activity snapshot, not client claims.
    pub actor: Option<&'a State>,
}

pub struct Policy {
    rules: Vec<Rule>,
}

impl Policy {
    pub fn compile(mut rules: Vec<Rule>) -> Result<Self, AdmissionError> {
        if rules.len() > MAX_NETWORK_RULES {
            return Err(AdmissionError::InvalidPolicy);
        }
        let mut ids = std::collections::HashSet::new();
        for rule in &rules {
            if rule.id < 1
                || !ids.insert(rule.id)
                || (rule.range.is_none() && rule.asn.is_none())
                || rule.asn == Some(0)
                || rule.range.is_some_and(|(first, last)| first > last)
                || rule.boards.len() > MAX_SCOPE_BOARDS * 11
                || rule.boards.split(',').count() > MAX_SCOPE_BOARDS
                || rule.boards.split(',').any(|board| {
                    !board.is_empty() && board != "_ws_" && BoardSlug::parse(board).is_err()
                })
                || rule.signatures.len() > MAX_SIGNATURE_LIST_BYTES
                || rule.signatures.chars().any(char::is_control)
            {
                return Err(AdmissionError::InvalidPolicy);
            }
        }
        // Source ordering is leniency ascending. Preserve the supplied explicit
        // ordering for ties; the store supplies a stable operator rule order.
        rules.sort_by_key(|rule| rule.leniency);
        Ok(Self { rules })
    }

    pub fn evaluate(&self, request: Request<'_>) -> Result<Option<&Rule>, AdmissionError> {
        if request
            .board
            .is_some_and(|board| BoardSlug::parse(board).is_err())
            || matches!(request.asn, ProviderValue::Available(Some(0)))
            || match request.signatures {
                ProviderValue::Unavailable => false,
                ProviderValue::Available(hints) => [hints.browser_id, hints.request_signature]
                    .into_iter()
                    .flatten()
                    .any(|value| {
                        value.len() > MAX_SIGNATURE_BYTES || value.chars().any(char::is_control)
                    }),
            }
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        for rule in &self.rules {
            let range_match = rule.range.is_some_and(|(first, last)| {
                request
                    .address
                    .is_some_and(|address| first <= address && address <= last)
            });
            if !rule.active
                || (rule.expires_at != 0 && rule.expires_at <= request.now)
                || (php_truthy(&rule.boards)
                    && !request.board.is_some_and(|board| {
                        rule.boards
                            .split(',')
                            .any(|scope| scope == board || (scope == "_ws_" && request.work_safe))
                    }))
                || (rule.reports_only && !request.report)
                || (rule.ops_only && !request.op)
                || (rule.images_only && !request.image)
            {
                continue;
            }
            if !range_match {
                let Some(asn) = rule.asn else {
                    continue;
                };
                match request.asn {
                    ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
                    ProviderValue::Available(value) if value == Some(asn) => {}
                    ProviderValue::Available(_) => continue,
                }
            }
            if php_truthy(&rule.signatures) {
                let hints = match request.signatures {
                    ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
                    ProviderValue::Available(hints) => hints,
                };
                if ![hints.browser_id, hints.request_signature]
                    .into_iter()
                    .flatten()
                    .any(|signature| php_truthy(signature) && rule.signatures.contains(signature))
                {
                    continue;
                }
            }
            if let Some(actor) = request.actor {
                let posts = actor.post_count();
                let post_count_ok = !(rule.ops_only || rule.images_only)
                    || (posts >= 3
                        && (actor.network_age(request.activity_now.unwrap_or(request.now)) > 900
                            || posts >= 15));
                let since = if rule.updated_at != 0 {
                    rule.updated_at
                } else {
                    rule.created_at
                };
                let known =
                    actor.is_known(request.activity_now.unwrap_or(request.now), 1_440, since)
                        && post_count_ok;
                let verified = actor.verified_level != 0;
                let exempt = match rule.leniency {
                    Leniency::None => false,
                    Leniency::KnownOrVerified => verified || known,
                    Leniency::Known => known,
                    Leniency::Verified => verified,
                };
                if exempt {
                    continue;
                }
            }
            return Ok(Some(rule));
        }
        Ok(None)
    }
}

fn php_truthy(value: &str) -> bool {
    !value.is_empty() && value != "0"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn number(value: &Value, key: &str) -> u64 {
        value[key].as_u64().unwrap()
    }
    fn rule(value: &Value) -> Rule {
        let asn = number(value, "asn") as u32;
        Rule {
            id: value["id"].as_i64().unwrap(),
            range: Some((
                Ipv4Addr::from(number(value, "range_start") as u32),
                Ipv4Addr::from(number(value, "range_end") as u32),
            )),
            asn: (asn != 0).then_some(asn),
            active: number(value, "active") != 0,
            expires_at: number(value, "expires_on"),
            created_at: number(value, "created_on"),
            updated_at: number(value, "updated_on"),
            boards: value["boards"].as_str().unwrap().into(),
            ops_only: number(value, "ops_only") != 0,
            images_only: number(value, "img_only") != 0,
            reports_only: number(value, "report_only") != 0,
            leniency: match number(value, "lenient") {
                0 => Leniency::None,
                1 => Leniency::KnownOrVerified,
                2 => Leniency::Known,
                3 => Leniency::Verified,
                _ => panic!("Unknown fixture leniency"),
            },
            signatures: value["ua_ids"].as_str().unwrap().into(),
        }
    }

    #[test]
    fn network_policy_matches_executed_source_with_real_activity_methods() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/network-admission-reference.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1392);
        for (index, case) in cases.iter().enumerate() {
            let now = number(case, "now");
            let actor = if case["actor"].is_null() {
                None
            } else {
                let a = &case["actor"];
                let mut state = State::new(now);
                state.created_at = now - number(a, "password_age");
                state.network_at = now - number(a, "network_age");
                state.posts = number(a, "posts") as u8;
                state.reports = number(a, "reports") as u8;
                state.pending = number(a, "pending") as u8;
                state.verified_level = number(a, "verified") as u8;
                state.change_score = number(a, "score") as u8;
                Some(state)
            };
            let policy =
                Policy::compile(case["rules"].as_array().unwrap().iter().map(rule).collect())
                    .unwrap();
            let asn = number(case, "asn") as u32;
            let matched = policy
                .evaluate(Request {
                    now,
                    activity_now: None,
                    address: Some(Ipv4Addr::from(number(case, "address") as u32)),
                    asn: ProviderValue::Available((asn != 0).then_some(asn)),
                    board: case["board"].as_str(),
                    work_safe: case["is_sfw"].as_bool().unwrap(),
                    op: case["is_op"].as_bool().unwrap(),
                    image: case["has_img"].as_bool().unwrap(),
                    report: case["is_report"].as_bool().unwrap(),
                    signatures: ProviderValue::Available(Signatures {
                        browser_id: case["browser_id"].as_str(),
                        request_signature: case["req_sig"].as_str(),
                    }),
                    actor: actor.as_ref(),
                })
                .unwrap()
                .map(|row| row.id);
            assert_eq!(matched, case["expected_id"].as_i64(), "source case {index}");
        }
    }

    #[test]
    fn request_bounds_and_asn_only_selectors_are_explicit() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/network-admission-reference.json"
        ))
        .unwrap();
        let mut entry = rule(&fixture["cases"][0]["rules"][0]);
        entry.range = None;
        entry.asn = Some(64512);
        let policy = Policy::compile(vec![entry]).unwrap();
        let request = || Request {
            now: 1_700_000_000,
            activity_now: None,
            address: None,
            asn: ProviderValue::Available(Some(64512)),
            board: Some("g"),
            work_safe: false,
            op: false,
            image: false,
            report: false,
            signatures: ProviderValue::Available(Signatures {
                browser_id: None,
                request_signature: None,
            }),
            actor: None,
        };
        assert_eq!(
            policy.evaluate(request()).unwrap().map(|row| row.id),
            Some(1)
        );
        let mut missing = request();
        missing.asn = ProviderValue::Available(None);
        assert!(policy.evaluate(missing).unwrap().is_none());
        let long = "a".repeat(MAX_SIGNATURE_BYTES + 1);
        for (browser, signature, board) in [
            (Some(long.as_str()), None, Some("g")),
            (None, Some(long.as_str()), Some("g")),
            (Some("bad\nvalue"), None, Some("g")),
            (None, None, Some("../outside")),
        ] {
            let mut invalid = request();
            invalid.signatures = ProviderValue::Available(Signatures {
                browser_id: browser,
                request_signature: signature,
            });
            invalid.board = board;
            assert!(matches!(
                policy.evaluate(invalid),
                Err(AdmissionError::InvalidPolicy)
            ));
        }
    }

    #[test]
    fn unavailable_providers_fail_only_at_an_applicable_ordered_stage() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/network-admission-reference.json"
        ))
        .unwrap();
        let base = &fixture["cases"][0]["rules"][0];
        let request = || Request {
            now: 1_700_000_000,
            activity_now: None,
            address: Some(Ipv4Addr::new(192, 0, 2, 10)),
            asn: ProviderValue::Unavailable,
            board: Some("g"),
            work_safe: false,
            op: false,
            image: false,
            report: false,
            signatures: ProviderValue::Unavailable,
            actor: None,
        };
        for change in 0..7 {
            let mut entry = rule(base);
            entry.range = None;
            entry.asn = Some(64512);
            match change {
                0 => {}
                1 => entry.reports_only = true,
                2 => entry.ops_only = true,
                3 => entry.images_only = true,
                4 => entry.boards = "a".into(),
                5 => entry.expires_at = 1,
                6 => entry.active = false,
                _ => unreachable!(),
            }
            let policy = Policy::compile(vec![entry]).unwrap();
            let result = policy.evaluate(request());
            if change == 0 {
                assert!(matches!(result, Err(AdmissionError::Unavailable)));
            } else {
                assert!(result.unwrap().is_none(), "inapplicable case {change}");
            }
        }
        let mut later = rule(base);
        later.id = 2;
        later.range = None;
        later.asn = Some(64512);
        let policy = Policy::compile(vec![rule(base), later]).unwrap();
        assert_eq!(policy.evaluate(request()).unwrap().map(|r| r.id), Some(1));
        for signature in ["", "0", "known-hint"] {
            let mut entry = rule(base);
            entry.signatures = signature.into();
            let policy = Policy::compile(vec![entry]).unwrap();
            let result = policy.evaluate(request());
            if signature == "known-hint" {
                assert!(matches!(result, Err(AdmissionError::Unavailable)));
            } else {
                assert_eq!(result.unwrap().map(|r| r.id), Some(1));
            }
        }
    }

    #[test]
    fn activity_age_uses_its_snapshot_clock_without_changing_rule_expiry() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/network-admission-reference.json"
        ))
        .unwrap();
        let now = 1_700_000_000;
        let mut entry = rule(&fixture["cases"][0]["rules"][0]);
        entry.ops_only = true;
        entry.leniency = Leniency::Known;
        entry.created_at = now - 899;
        entry.expires_at = now + 1;
        let policy = Policy::compile(vec![entry]).unwrap();
        let mut actor = State::new(now - 3600);
        actor.network_at = now - 900;
        actor.posts = 3;
        let request = |activity_now, actor| Request {
            now,
            activity_now,
            address: Some(Ipv4Addr::new(192, 0, 2, 10)),
            asn: ProviderValue::Available(None),
            board: Some("g"),
            work_safe: false,
            op: true,
            image: false,
            report: false,
            signatures: ProviderValue::Available(Signatures {
                browser_id: None,
                request_signature: None,
            }),
            actor,
        };
        assert_eq!(
            policy
                .evaluate(request(None, Some(&actor)))
                .unwrap()
                .map(|r| r.id),
            Some(1)
        );
        assert!(
            policy
                .evaluate(request(Some(now + 1), Some(&actor)))
                .unwrap()
                .is_none()
        );
        // A later activity clock does not expire a rule at the earlier request clock.
        assert_eq!(
            policy
                .evaluate(request(Some(now + 1), None))
                .unwrap()
                .map(|r| r.id),
            Some(1)
        );
        let mut expired = request(Some(now + 1), None);
        expired.now += 1;
        assert!(policy.evaluate(expired).unwrap().is_none());
    }

    #[test]
    fn invalid_operator_scope_and_work_are_rejected() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/network-admission-reference.json"
        ))
        .unwrap();
        let base = &fixture["cases"][0]["rules"][0];
        for change in 0..8 {
            let mut entry = rule(base);
            match change {
                0 => entry.id = 0,
                1 => entry.range = Some((Ipv4Addr::BROADCAST, Ipv4Addr::UNSPECIFIED)),
                2 => {
                    entry.range = None;
                    entry.asn = None;
                }
                3 => entry.asn = Some(0),
                4 => entry.boards = "../outside".into(),
                5 => entry.boards = vec!["g"; MAX_SCOPE_BOARDS + 1].join(","),
                6 => entry.signatures = "a".repeat(MAX_SIGNATURE_LIST_BYTES + 1),
                7 => entry.signatures = "bad\nvalue".into(),
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    Policy::compile(vec![entry]),
                    Err(AdmissionError::InvalidPolicy)
                ),
                "invalid case {change}"
            );
        }
        assert!(Policy::compile(vec![rule(base), rule(base)]).is_err());
        assert!(
            Policy::compile(
                (0..=MAX_NETWORK_RULES)
                    .map(|index| {
                        let mut entry = rule(base);
                        entry.id = index as i64 + 1;
                        entry
                    })
                    .collect()
            )
            .is_err()
        );
    }
}
