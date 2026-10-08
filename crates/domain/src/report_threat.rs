//! Bounded positive proofs for the reporting threat threshold, not a scorer.
//!
//! `4chan-old/lib/postfilter.php:2094–2097` adds 1 for its non-browser UA
//! expression; lines 2122–2124 add 0.8 for any of three request-origin server
//! fields being set. The source sums nonnegative additions, multiplies by
//! factors at least one, then rounds to two decimals (2303–2315). Either signal
//! therefore proves the final score is at least 0.4 without knowing its total.
//! Missing these signals does not prove a low score: other terms are unmodeled.
//! No HTTP extraction, identity inference, runtime integration, or persistence.

use crate::report_weight::Evidence;

#[derive(Clone, Copy, Debug)]
pub struct ThreatFacts<'a> {
    /// The complete source HTTP_USER_AGENT value, or qualified source absence.
    /// A truncated value must be Unknown. Only wholly ASCII values are matched.
    pub user_agent: Evidence<Option<&'a [u8]>>,
    /// Qualified source `isset($_SERVER['HTTP_PATH'])`, including empty values.
    /// This is a request-origin field, not the URL path.
    pub http_path_is_set: Evidence<bool>,
    /// Qualified source `isset($_SERVER['HTTP_SAME_ORIGIN'])`.
    pub http_same_origin_is_set: Evidence<bool>,
    /// Qualified source `isset($_SERVER['HTTP_REFERRER_POLICY'])`.
    /// This is a request-origin field, not a response policy setting.
    pub http_referrer_policy_is_set: Evidence<bool>,
}

/// Positive witnesses only; a missing witness is not a negative assertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThresholdProof {
    NonBrowserUserAgent,
    RequestHeaderPresence,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreatDecision {
    Unknown,
    AtLeastPointFour { proof: ThresholdProof },
}

impl ThreatDecision {
    /// Preserve unknowns and never synthesize a qualified below-threshold fact.
    pub fn weight_evidence(self) -> Evidence<bool> {
        match self {
            Self::Unknown => Evidence::Unknown,
            Self::AtLeastPointFour { .. } => Evidence::Qualified(true),
        }
    }
}

fn matches_non_browser_ua(value: &[u8]) -> bool {
    if !value.is_ascii() {
        return false;
    }
    // Exact ASCII case-insensitive substring alternatives from the source:
    // /headless|node-fetch|python-|java\/|jakarta|-perl|http-?client|-resty-|awesomium\//i
    const PATTERNS: &[&[u8]] = &[
        b"headless",
        b"node-fetch",
        b"python-",
        b"java/",
        b"jakarta",
        b"-perl",
        b"httpclient",
        b"http-client",
        b"-resty-",
        b"awesomium/",
    ];
    PATTERNS.iter().any(|pattern| {
        value
            .windows(pattern.len())
            .any(|window| window.eq_ignore_ascii_case(pattern))
    })
}

pub fn prove_report_threat(facts: ThreatFacts<'_>) -> ThreatDecision {
    let ua_proven = match facts.user_agent {
        Evidence::Qualified(Some(value)) => matches_non_browser_ua(value),
        Evidence::Unknown | Evidence::Qualified(None) => false,
    };
    let header_proven = [
        facts.http_path_is_set,
        facts.http_same_origin_is_set,
        facts.http_referrer_policy_is_set,
    ]
    .contains(&Evidence::Qualified(true));
    let proof = match (ua_proven, header_proven) {
        (false, false) => return ThreatDecision::Unknown,
        (true, false) => ThresholdProof::NonBrowserUserAgent,
        (false, true) => ThresholdProof::RequestHeaderPresence,
        (true, true) => ThresholdProof::Both,
    };
    ThreatDecision::AtLeastPointFour { proof }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_weight::{Finite, WeightDecision, WeightFacts, decide_weight};
    use Evidence::{Qualified as Q, Unknown as U};

    fn facts() -> ThreatFacts<'static> {
        ThreatFacts {
            user_agent: U,
            http_path_is_set: U,
            http_same_origin_is_set: U,
            http_referrer_policy_is_set: U,
        }
    }

    #[test]
    fn exact_ascii_regex_alternatives_are_case_insensitive_substrings() {
        for pattern in [
            "headless",
            "node-fetch",
            "python-",
            "java/",
            "jakarta",
            "-perl",
            "httpclient",
            "http-client",
            "-resty-",
            "awesomium/",
        ] {
            for ua in [
                pattern.to_owned(),
                pattern.to_ascii_uppercase(),
                format!("pre{pattern}post"),
            ] {
                assert_eq!(
                    prove_report_threat(ThreatFacts {
                        user_agent: Q(Some(ua.as_bytes())),
                        ..facts()
                    }),
                    ThreatDecision::AtLeastPointFour {
                        proof: ThresholdProof::NonBrowserUserAgent
                    }
                );
            }
        }
    }

    #[test]
    fn near_misses_absence_unknown_and_nonascii_do_not_prove_low_or_high() {
        for ua in [
            "",
            "Mozilla/5.0",
            "headles",
            "nodefetch",
            "python",
            "java",
            "jakart",
            "perl",
            "http--client",
            "http_client",
            "resty",
            "-resty",
            "awesomium",
            "HEADLESSé",
            "éhttpclient",
        ] {
            let decision = prove_report_threat(ThreatFacts {
                user_agent: Q(Some(ua.as_bytes())),
                ..facts()
            });
            assert_eq!(decision, ThreatDecision::Unknown, "{ua}");
            assert_eq!(decision.weight_evidence(), U);
        }
        for ua in [U, Q(None)] {
            assert_eq!(
                prove_report_threat(ThreatFacts {
                    user_agent: ua,
                    http_path_is_set: Q(false),
                    http_same_origin_is_set: Q(false),
                    http_referrer_policy_is_set: Q(false),
                }),
                ThreatDecision::Unknown
            );
        }
        assert_eq!(prove_report_threat(facts()), ThreatDecision::Unknown);
    }

    #[test]
    fn header_isset_presence_is_sufficient_despite_other_unknowns() {
        for path in [U, Q(false), Q(true)] {
            for origin in [U, Q(false), Q(true)] {
                for policy in [U, Q(false), Q(true)] {
                    let decision = prove_report_threat(ThreatFacts {
                        // Non-ASCII UA must not prevent an independent header proof.
                        user_agent: Q(Some(b"headless\xff")),
                        http_path_is_set: path,
                        http_same_origin_is_set: origin,
                        http_referrer_policy_is_set: policy,
                    });
                    if [path, origin, policy].contains(&Q(true)) {
                        assert_eq!(
                            decision,
                            ThreatDecision::AtLeastPointFour {
                                proof: ThresholdProof::RequestHeaderPresence,
                            }
                        );
                        assert_eq!(decision.weight_evidence(), Q(true));
                    } else {
                        assert_eq!(decision, ThreatDecision::Unknown);
                    }
                }
            }
        }
        // Qualified isset is true even for an empty string. No HTTP adapter
        // is provided here: the caller must establish source presence itself.
        let source_value: Option<&str> = Some("");
        assert_eq!(
            prove_report_threat(ThreatFacts {
                http_path_is_set: Q(source_value.is_some()),
                ..facts()
            }),
            ThreatDecision::AtLeastPointFour {
                proof: ThresholdProof::RequestHeaderPresence
            }
        );
    }

    #[test]
    fn positive_proofs_compose_with_weight_without_resolving_staff() {
        let threat = prove_report_threat(ThreatFacts {
            user_agent: Q(Some(b"python-requests")),
            http_same_origin_is_set: Q(true),
            ..facts()
        });
        assert_eq!(
            threat,
            ThreatDecision::AtLeastPointFour {
                proof: ThresholdProof::Both
            }
        );
        let input = WeightFacts {
            category_base: Q(Finite::new(2.0).unwrap()),
            authenticated_janitor_or_higher: U,
            known_or_verified: U,
            threat_at_least_point_four: threat.weight_evidence(),
            category_filtered: U,
            history_filtered: U,
        };
        assert_eq!(decide_weight(input), WeightDecision::Unknown);
        match decide_weight(WeightFacts {
            authenticated_janitor_or_higher: Q(false),
            ..input
        }) {
            WeightDecision::Qualified {
                weight,
                source_reason,
                ..
            } => {
                assert_eq!(weight.get(), 0.5);
                assert_eq!(source_reason, None);
            }
            WeightDecision::Unknown => panic!("proven nonstaff threshold must establish fallback"),
        }
        assert_eq!(
            decide_weight(WeightFacts {
                authenticated_janitor_or_higher: Q(false),
                threat_at_least_point_four: ThreatDecision::Unknown.weight_evidence(),
                ..input
            }),
            WeightDecision::Unknown
        );
    }
}
