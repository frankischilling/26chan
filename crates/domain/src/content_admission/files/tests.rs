use super::*;
use ProviderValue::{Available, Unavailable};
use serde_json::Value;

fn digest(value: &Value) -> Option<Digest> {
    value.as_str().map(|hex| {
        assert_eq!(hex.len(), 32);
        std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
    })
}
fn hex(value: Digest) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn rule(id: i64, digest: Digest, mode: Mode) -> Rule {
    Rule {
        id,
        digest,
        mode,
        scope: String::new(),
        active: true,
        quiet: false,
    }
}
fn request() -> Request<'static> {
    Request {
        board: "g",
        worksafe: true,
        test_board: false,
        badged: false,
        normalized: Available(Some([1; 16])),
        original: Available(Some([2; 16])),
        original_eligible: Available(true),
        prior_dmca: Available(false),
    }
}

#[test]
fn exact_decisions_match_selected_source_function() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/file-filter-reference.json"
    ))
    .unwrap();
    let cases = fixture["exact_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2358);
    let mut unavailable_cases = 0;
    for (index, case) in cases.iter().enumerate() {
        let rules = case["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| Rule {
                id: row["id"].as_i64().unwrap(),
                scope: row["boardrestrict"].as_str().unwrap().into(),
                digest: digest(&row["contents"]).unwrap(),
                active: row["active"].as_bool().unwrap(),
                quiet: row["quiet"].as_bool().unwrap(),
                mode: match row["ban"].as_i64().unwrap() {
                    0 => Mode::Reject,
                    1 => Mode::Ban {
                        days: row["banlength"].as_i64().unwrap().try_into().unwrap(),
                    },
                    2 => Mode::Dmca,
                    _ => panic!("unexpected reference mode"),
                },
            })
            .collect();
        let policy = Policy::compile(rules).unwrap();
        let outcome = policy.evaluate(Request {
            board: case["board"].as_str().unwrap(),
            worksafe: case["worksafe"].as_bool().unwrap(),
            test_board: case["test_board"].as_bool().unwrap(),
            badged: false,
            normalized: Available(digest(&case["normalized"])),
            original: Available(digest(&case["original"])),
            original_eligible: Available(true),
            prior_dmca: if case["dmca_database_failure"].as_bool().unwrap() {
                Unavailable
            } else {
                Available(case["prior_dmca"].as_bool().unwrap())
            },
        });
        let expected = &case["result"];
        if case["dmca_database_failure"].as_bool().unwrap() {
            assert_eq!(expected["message"], "dmca-warning");
            assert_eq!(case["effects"][0]["kind"], "dmca-warning");
            assert!(matches!(outcome, Err(AdmissionError::Unavailable)));
            unavailable_cases += 1;
            continue;
        }
        let outcome = outcome.unwrap();
        if expected["outcome"] == "allow" {
            assert!(outcome.is_none(), "case {index}");
            assert!(case["effects"].as_array().unwrap().is_empty());
            continue;
        }
        let outcome = outcome.unwrap_or_else(|| panic!("case {index} unexpectedly allowed"));
        assert_eq!(
            outcome.rule,
            case["matched"]["rule"].as_i64().unwrap(),
            "case {index}"
        );
        assert_eq!(
            hex(outcome.digest),
            case["matched"]["digest"].as_str().unwrap(),
            "case {index}"
        );
        let response = match expected["outcome"].as_str().unwrap() {
            "private-reject" => {
                assert_eq!(
                    expected["message"],
                    format!(
                        "Blacklisted md5 - {} - Filename: fixture.png",
                        hex(outcome.digest)
                    )
                );
                Response::PrivateDiagnostic
            }
            "quiet" => {
                assert_eq!(expected["parent"], 7);
                Response::Quiet
            }
            "redirect" => Response::BanRedirect,
            "reject" if expected["message"] == "dmca-warning" => Response::DmcaWarning,
            "reject" if expected["message"] == "failed-upload" => Response::FailedUpload,
            _ => panic!("unexpected reference response"),
        };
        assert_eq!(outcome.response, response, "case {index}");
        let effects = case["effects"].as_array().unwrap();
        let effect = match effects.as_slice() {
            [] => Effect::None,
            [effect] if effect["kind"] == "dmca-warning" => Effect::RecordDmcaWarning,
            [effect] if effect["kind"] == "ban" => Effect::Ban {
                days: effect["days"].as_i64().unwrap().try_into().unwrap(),
                dmca_repeat: effect["filter"].as_bool().unwrap(),
            },
            _ => panic!("unexpected reference effect"),
        };
        assert_eq!(outcome.effect, effect, "case {index}");
    }
    assert_eq!(unavailable_cases, 24);
}

#[test]
fn prior_ban_thresholds_match_source_with_explicit_failure_difference() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/file-filter-reference.json"
    ))
    .unwrap();
    let cases = fixture["prior_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 600);
    let mut unavailable = 0;
    for case in cases {
        let failure = case["database_failure"].as_bool().unwrap();
        let board = case["board"].as_str().unwrap();
        let normalized = digest(&case["normalized"]);
        let result = prior_ban_rejection(
            board,
            case["worksafe"].as_bool().unwrap(),
            Available(normalized),
            if failure {
                Unavailable
            } else {
                Available(PriorBanCounts {
                    digest: normalized.unwrap_or([0; 16]),
                    template_226: case["count_226"].as_u64().unwrap(),
                    template_6: case["count_6"].as_u64().unwrap(),
                })
            },
        );
        if failure && board != "b" && normalized.is_some() {
            assert_eq!(
                case["blocked"], false,
                "Source skips a failed history query"
            );
            assert!(matches!(result, Err(AdmissionError::Unavailable)));
            unavailable += 1;
        } else {
            assert_eq!(result.unwrap(), case["blocked"].as_bool().unwrap());
        }
    }
    assert_eq!(unavailable, 100);
    assert!(matches!(
        prior_ban_rejection(
            "g",
            true,
            Available(Some([1; 16])),
            Available(PriorBanCounts {
                digest: [2; 16],
                template_226: 3,
                template_6: 0,
            })
        ),
        Err(AdmissionError::InvalidPolicy)
    ));
    assert!(
        prior_ban_rejection(
            "g",
            true,
            Available(Some([1; 16])),
            Available(PriorBanCounts {
                digest: [1; 16],
                template_226: u64::MAX,
                template_6: u64::MAX,
            })
        )
        .unwrap()
    );
}

#[test]
fn only_relevant_unavailable_providers_block_a_decision() {
    let primary = Policy::compile(vec![rule(1, [1; 16], Mode::Reject)]).unwrap();
    let blocked = primary
        .evaluate(Request {
            original: Unavailable,
            original_eligible: Unavailable,
            prior_dmca: Unavailable,
            ..request()
        })
        .unwrap()
        .unwrap();
    assert_eq!(blocked.response, Response::FailedUpload);
    let original = Policy::compile(vec![rule(1, [2; 16], Mode::Reject)]).unwrap();
    assert!(matches!(
        original.evaluate(Request {
            original_eligible: Unavailable,
            ..request()
        }),
        Err(AdmissionError::Unavailable)
    ));
    assert!(
        original
            .evaluate(Request {
                original_eligible: Available(false),
                original: Unavailable,
                ..request()
            })
            .unwrap()
            .is_none()
    );
    assert!(
        original
            .evaluate(Request {
                original: Available(Some([3; 16])),
                original_eligible: Unavailable,
                ..request()
            })
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        original.evaluate(Request {
            original: Unavailable,
            ..request()
        }),
        Err(AdmissionError::Unavailable)
    ));
    assert!(matches!(
        original.evaluate(Request {
            normalized: Unavailable,
            ..request()
        }),
        Err(AdmissionError::Unavailable)
    ));
    let dmca = Policy::compile(vec![rule(1, [1; 16], Mode::Dmca)]).unwrap();
    assert!(matches!(
        dmca.evaluate(Request {
            prior_dmca: Unavailable,
            ..request()
        }),
        Err(AdmissionError::Unavailable)
    ));
    assert!(
        dmca.evaluate(Request {
            badged: true,
            normalized: Unavailable,
            prior_dmca: Unavailable,
            ..request()
        })
        .unwrap()
        .is_none()
    );
    assert!(
        Policy::compile(vec![])
            .unwrap()
            .evaluate(Request {
                normalized: Unavailable,
                ..request()
            })
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        prior_ban_rejection("g", true, Available(Some([1; 16])), Unavailable),
        Err(AdmissionError::Unavailable)
    ));
    assert!(!prior_ban_rejection("b", true, Unavailable, Unavailable).unwrap());
}

#[test]
fn ambiguous_scopes_and_unbounded_configuration_are_refused() {
    let mut local = rule(2, [1; 16], Mode::Dmca);
    local.scope = "g".into();
    let policy = Policy::compile(vec![rule(1, [1; 16], Mode::Reject), local]).unwrap();
    assert!(matches!(
        policy.evaluate(request()),
        Err(AdmissionError::InvalidPolicy)
    ));
    assert!(
        Policy::compile(vec![
            rule(1, [1; 16], Mode::Reject),
            rule(1, [2; 16], Mode::Reject)
        ])
        .is_err()
    );
    assert!(
        Policy::compile(
            (1..=129)
                .map(|id| rule(id, [1; 16], Mode::Reject))
                .collect()
        )
        .is_err()
    );
    for days in [-2, 3651, i32::MAX] {
        assert!(Policy::compile(vec![rule(1, [1; 16], Mode::Ban { days })]).is_err());
    }
    let mut invalid = rule(1, [1; 16], Mode::Reject);
    invalid.scope = "g,b".into();
    assert!(Policy::compile(vec![invalid]).is_err());
}

#[test]
fn candidate_matching_does_not_invent_a_dmca_warning_state() {
    let policy = Policy::compile(vec![Rule {
        id: 192,
        scope: String::new(),
        digest: [7; 16],
        active: true,
        quiet: true,
        mode: Mode::Dmca,
    }])
    .unwrap();
    let request = || Request {
        board: "g",
        worksafe: false,
        test_board: false,
        badged: false,
        normalized: ProviderValue::Available(Some([7; 16])),
        original: ProviderValue::Unavailable,
        original_eligible: ProviderValue::Unavailable,
        prior_dmca: ProviderValue::Unavailable,
    };
    let found = policy.match_rule(request()).unwrap().unwrap();
    assert_eq!(found.rule, 192);
    assert_eq!(found.digest, [7; 16]);
    assert!(matches!(
        policy.evaluate(request()),
        Err(AdmissionError::Unavailable)
    ));
}
