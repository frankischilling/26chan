//! Pure report-weight reasoning for `4chan-old/modes/report.php:568–611`.
//!
//! Callers must supply already-qualified legacy-equivalent facts. Actor hashes,
//! database roles, admission success, and session age alone establish none of
//! these facts. In particular, known-or-verified means the source's
//! `isUserKnownOrVerified(60)`, and staff means authenticated janitor-or-higher.
//! No serialization, persistence, runtime integration, or clearance is provided.

/// Missing evidence is distinct from a qualified false value. This type is
/// intentionally not serializable; it is not a client assertion or authority token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence<T> {
    Unknown,
    Qualified(T),
}

/// Finite numeric representation, without invented nonnegative or unit bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finite(f64);

impl Finite {
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }

    /// Convert an already-qualified exact threat score to the source predicate.
    /// A partial score must not use this helper to establish a false predicate.
    pub fn threat_at_least_point_four(self) -> bool {
        self.0 >= 0.4
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WeightFacts {
    pub category_base: Evidence<Finite>,
    pub authenticated_janitor_or_higher: Evidence<bool>,
    pub known_or_verified: Evidence<bool>,
    /// Qualified proof of the source predicate, not necessarily an exact score.
    /// A source-valid lower bound can prove true without inventing a total.
    /// False requires proof that the final source score is below 0.4; absence
    /// of a known positive signal is not such proof.
    pub threat_at_least_point_four: Evidence<bool>,
    /// Decoded integer category configuration, not a filtering verdict.
    pub category_filtered: Evidence<i64>,
    /// Qualified history-match predicate conditional on a positive configured
    /// threshold, using the source's identity/history rules. This is not an
    /// unconditional return value from the source helper: configuration is
    /// modeled separately by category_filtered. With unknown configuration,
    /// a true history match does not establish that filtering is enabled.
    /// A Pass ID or a database role alone is not this predicate.
    pub history_filtered: Evidence<bool>,
}

/// Exact source branch, only returned when every completion agrees on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceReason {
    StaffKeepsBase,
    KeepsBase,
    NotKnownOrVerified,
    ThreatAtLeastPointFour,
    HistoryFiltered,
}

impl SourceReason {
    fn uses_fallback(self) -> bool {
        matches!(
            self,
            Self::NotKnownOrVerified | Self::ThreatAtLeastPointFour | Self::HistoryFiltered
        )
    }
}

/// Numeric proof is deliberately independent of the source's ordered reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericProof {
    /// Every possible branch uses the category base (which is qualified).
    AllBranchesKeepBase,
    /// Every possible branch uses 0.5; their ignore reasons may differ.
    AllBranchesUseFallback,
    /// Some branches keep base and some fall back, but base is exactly 0.5.
    BaseEqualsFallback,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeightDecision {
    Unknown,
    Qualified {
        weight: Finite,
        proof: NumericProof,
        source_reason: Option<SourceReason>,
    },
}

fn possibilities(value: Evidence<bool>) -> &'static [bool] {
    match value {
        Evidence::Unknown => &[false, true],
        Evidence::Qualified(false) => &[false],
        Evidence::Qualified(true) => &[true],
    }
}

/// Return a numeric weight only if all completions of missing evidence agree.
/// An unknown earlier ignore condition does not mask a later proven fallback,
/// but it prevents attributing the result to that later source reason.
pub fn decide_weight(facts: WeightFacts) -> WeightDecision {
    // The helper at report.php:400 returns false for a threshold below 1.
    // This also handles negative, truthy PHP category configuration correctly.
    let filter = match (facts.category_filtered, facts.history_filtered) {
        (Evidence::Qualified(value), _) if value <= 0 => Evidence::Qualified(false),
        (_, Evidence::Qualified(false)) => Evidence::Qualified(false),
        (Evidence::Qualified(_), result) => result,
        (Evidence::Unknown, _) => Evidence::Unknown,
    };
    let mut first_reason = None;
    let mut reasons_agree = true;
    let mut can_keep_base = false;
    let mut can_fall_back = false;
    for &staff in possibilities(facts.authenticated_janitor_or_higher) {
        for &known in possibilities(facts.known_or_verified) {
            for &high_threat in possibilities(facts.threat_at_least_point_four) {
                for &filtered in possibilities(filter) {
                    let reason = if staff {
                        SourceReason::StaffKeepsBase
                    } else if !known {
                        SourceReason::NotKnownOrVerified
                    } else if high_threat {
                        SourceReason::ThreatAtLeastPointFour
                    } else if filtered {
                        SourceReason::HistoryFiltered
                    } else {
                        SourceReason::KeepsBase
                    };
                    if let Some(first) = first_reason {
                        reasons_agree &= first == reason;
                    } else {
                        first_reason = Some(reason);
                    }
                    can_fall_back |= reason.uses_fallback();
                    can_keep_base |= !reason.uses_fallback();
                }
            }
        }
    }
    let (weight, proof) = if !can_keep_base {
        (Finite(0.5), NumericProof::AllBranchesUseFallback)
    } else if let Evidence::Qualified(base) = facts.category_base {
        if !can_fall_back {
            (base, NumericProof::AllBranchesKeepBase)
        } else if base.get() == 0.5 {
            (base, NumericProof::BaseEqualsFallback)
        } else {
            return WeightDecision::Unknown;
        }
    } else {
        return WeightDecision::Unknown;
    };
    WeightDecision::Qualified {
        weight,
        proof,
        source_reason: if reasons_agree { first_reason } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Evidence::{Qualified as Q, Unknown as U};

    fn finite(value: f64) -> Finite {
        Finite::new(value).unwrap()
    }
    fn trits() -> [Evidence<bool>; 3] {
        [U, Q(false), Q(true)]
    }
    fn threshold(value: Evidence<bool>) -> Evidence<i64> {
        match value {
            U => U,
            Q(false) => Q(0),
            Q(true) => Q(1),
        }
    }

    // Deliberately independent of the production possibilities helper.
    fn completions(value: Evidence<bool>) -> Vec<bool> {
        match value {
            U => vec![true, false],
            Q(value) => vec![value],
        }
    }

    // Independent exact oracle: ordered imperative updates mirroring PHP.
    fn oracle(
        base: f64,
        staff: bool,
        known: bool,
        threat: bool,
        enabled: bool,
        history: bool,
    ) -> (f64, SourceReason) {
        if staff {
            return (base, SourceReason::StaffKeepsBase);
        }
        let mut reason = SourceReason::KeepsBase;
        if !known {
            reason = SourceReason::NotKnownOrVerified;
        } else if threat {
            reason = SourceReason::ThreatAtLeastPointFour;
        } else if enabled && history {
            reason = SourceReason::HistoryFiltered;
        }
        let weight = match reason {
            SourceReason::KeepsBase | SourceReason::StaffKeepsBase => base,
            _ => 0.5,
        };
        (weight, reason)
    }

    #[test]
    fn exhaustive_unknown_completions_match_exact_source_oracle() {
        for base in [
            U,
            Q(finite(-3.0)),
            Q(finite(0.0)),
            Q(finite(0.5)),
            Q(finite(2.0)),
        ] {
            for staff in trits() {
                for known in trits() {
                    for threat in trits() {
                        for enabled in trits() {
                            for history in trits() {
                                let decision = decide_weight(WeightFacts {
                                    category_base: base,
                                    authenticated_janitor_or_higher: staff,
                                    known_or_verified: known,
                                    threat_at_least_point_four: threat,
                                    category_filtered: threshold(enabled),
                                    history_filtered: history,
                                });
                                // Two distinct base completions suffice: each exact path
                                // returns either its unchanged base or the constant 0.5.
                                let bases = match base {
                                    U => vec![-7.0, 9.0],
                                    Q(v) => vec![v.get()],
                                };
                                let mut outcomes = Vec::new();
                                for b in bases {
                                    for &s in &completions(staff) {
                                        for &k in &completions(known) {
                                            for &t in &completions(threat) {
                                                for &e in &completions(enabled) {
                                                    for &h in &completions(history) {
                                                        outcomes.push(oracle(b, s, k, t, e, h));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                let numeric_agreement =
                                    outcomes.iter().all(|v| v.0 == outcomes[0].0);
                                let reason_agreement =
                                    outcomes.iter().all(|v| v.1 == outcomes[0].1);
                                match decision {
                                    WeightDecision::Unknown => assert!(!numeric_agreement),
                                    WeightDecision::Qualified {
                                        weight,
                                        source_reason,
                                        proof,
                                    } => {
                                        assert!(numeric_agreement);
                                        assert_eq!(weight.get(), outcomes[0].0);
                                        let base_branches = outcomes
                                            .iter()
                                            .filter(|(_, reason)| {
                                                matches!(
                                                    reason,
                                                    SourceReason::StaffKeepsBase
                                                        | SourceReason::KeepsBase
                                                )
                                            })
                                            .count();
                                        let expected_proof = if base_branches == outcomes.len() {
                                            NumericProof::AllBranchesKeepBase
                                        } else if base_branches == 0 {
                                            NumericProof::AllBranchesUseFallback
                                        } else {
                                            NumericProof::BaseEqualsFallback
                                        };
                                        assert_eq!(proof, expected_proof);
                                        assert_eq!(
                                            source_reason,
                                            reason_agreement.then_some(outcomes[0].1)
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn facts() -> WeightFacts {
        WeightFacts {
            category_base: Q(finite(-2.0)),
            authenticated_janitor_or_higher: Q(false),
            known_or_verified: Q(true),
            threat_at_least_point_four: Q(false),
            category_filtered: Q(0),
            history_filtered: U,
        }
    }
    fn weight(facts: WeightFacts) -> Option<f64> {
        match decide_weight(facts) {
            WeightDecision::Unknown => None,
            WeightDecision::Qualified { weight, .. } => Some(weight.get()),
        }
    }

    #[test]
    fn finite_values_threshold_edges_zero_and_negative_values() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Finite::new(bad).is_none());
        }
        for base in [-f64::MAX, -2.0, -0.0, 0.0, f64::MIN_POSITIVE, f64::MAX] {
            assert_eq!(
                weight(WeightFacts {
                    category_base: Q(finite(base)),
                    ..facts()
                }),
                Some(base)
            );
        }
        for (threat, expected) in [
            (-f64::MAX, -2.0),
            (f64::from_bits(0.4_f64.to_bits() - 1), -2.0),
            (0.4, 0.5),
            (f64::from_bits(0.4_f64.to_bits() + 1), 0.5),
            (f64::MAX, 0.5),
        ] {
            assert_eq!(
                weight(WeightFacts {
                    threat_at_least_point_four: Q(finite(threat).threat_at_least_point_four()),
                    ..facts()
                }),
                Some(expected)
            );
        }
        for threshold in [i64::MIN, -1, 0] {
            assert_eq!(
                weight(WeightFacts {
                    category_filtered: Q(threshold),
                    history_filtered: Q(true),
                    ..facts()
                }),
                Some(-2.0)
            );
        }
        for threshold in [1, i64::MAX] {
            assert_eq!(
                weight(WeightFacts {
                    category_filtered: Q(threshold),
                    history_filtered: Q(true),
                    ..facts()
                }),
                Some(0.5)
            );
        }
    }

    #[test]
    fn numeric_proof_does_not_invent_an_ordered_ignore_reason() {
        let input = WeightFacts {
            known_or_verified: U,
            threat_at_least_point_four: Q(true),
            ..facts()
        };
        assert_eq!(
            decide_weight(input),
            WeightDecision::Qualified {
                weight: finite(0.5),
                proof: NumericProof::AllBranchesUseFallback,
                source_reason: None
            }
        );
        assert_eq!(
            weight(WeightFacts {
                authenticated_janitor_or_higher: U,
                ..input
            }),
            None
        );
        assert_eq!(
            decide_weight(WeightFacts {
                authenticated_janitor_or_higher: U,
                category_base: Q(finite(0.5)),
                ..input
            }),
            WeightDecision::Qualified {
                weight: finite(0.5),
                proof: NumericProof::BaseEqualsFallback,
                source_reason: None
            }
        );
        assert_eq!(
            weight(WeightFacts {
                authenticated_janitor_or_higher: Q(true),
                ..input
            }),
            Some(-2.0)
        );
        assert_eq!(
            weight(WeightFacts {
                category_base: U,
                ..input
            }),
            Some(0.5)
        );
    }
}
