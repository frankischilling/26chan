use super::*;
use serde_json::{Value, json};

fn rule(value: &Value) -> Rule {
    Rule {
        id: value["id"].as_i64().unwrap(),
        board: value["board"].as_str().unwrap().into(),
        pattern: value["pattern"].as_str().unwrap().into(),
        regex: value["regex"].as_bool().unwrap(),
        autosage: value["autosage"].as_bool().unwrap(),
        log: value["log"].as_bool().unwrap(),
        quiet: value["quiet"].as_bool().unwrap(),
        lenient: value["lenient"].as_bool().unwrap(),
        ops_only: value["ops_only"].as_bool().unwrap(),
        min_count: value["min_count"].as_i64().unwrap() as i32,
        ban_days: value["ban_days"].as_i64().unwrap() as i32,
    }
}

fn ordinary(pattern: &str, regex: bool) -> Rule {
    Rule {
        id: 1,
        board: String::new(),
        pattern: pattern.into(),
        regex,
        autosage: false,
        log: false,
        quiet: false,
        lenient: false,
        ops_only: false,
        min_count: 1,
        ban_days: 0,
    }
}

fn post(comment: &str) -> Post<'_> {
    Post {
        board: "demo",
        reply: false,
        name: "Anonymous",
        subject: "",
        comment,
        filename: "",
    }
}

#[test]
fn native_invalid_unicode_cannot_become_an_empty_matching_projection() {
    let policy = Policy::compile(vec![ordinary("paper", false)]).unwrap();
    assert_eq!(
        policy.evaluate(post("\u{10000}\u{309d}"), Actor::default()),
        Err(AdmissionError::Normalization(
            NormalizationError::InvalidOutput
        ))
    );
    assert_eq!(
        policy.evaluate(post("\u{10000}"), Actor::default()),
        Ok(Decision::Allow)
    );
    assert_eq!(
        policy.evaluate(post("\u{20000}\u{309d}"), Actor::default()),
        Ok(Decision::Allow)
    );
}

#[test]
fn recorded_source_decisions_and_all_effects_match() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/content-admission.json")).unwrap();
    assert_eq!(fixture["extractor_icu"], "74.2");
    let cases = fixture["cases"].as_array().unwrap();
    let mut compared = 0;
    let mut unavailable = 0;
    for case in cases {
        let input = &case["input"];
        if !input["query_available"].as_bool().unwrap() {
            // The caller must return Unavailable before evaluation. Source
            // returns allow on DB failure; that security exception is explicit.
            assert_eq!(case["outcome"]["kind"], "allow");
            unavailable += 1;
            continue;
        }
        let policy = Policy::compile(
            input["rules"]
                .as_array()
                .unwrap()
                .iter()
                .map(rule)
                .collect(),
        )
        .unwrap();
        let post = Post {
            board: input["board"].as_str().unwrap(),
            reply: input["reply"].as_bool().unwrap(),
            name: input["name"].as_str().unwrap(),
            subject: input["subject"].as_str().unwrap(),
            comment: input["comment"].as_str().unwrap(),
            filename: input["filename"].as_str().unwrap(),
        };
        let actor = Actor {
            session_present: input["session"].as_bool().unwrap(),
            known_or_verified: input["known"].as_bool().unwrap(),
            posts: input["posts"].as_u64().unwrap() as u16,
        };
        let decision = policy.evaluate(post, actor).unwrap();
        let mut effects = json!({"hits": [], "logs": [], "bans": []});
        let mut kind = "allow";
        let mut message = None;
        let mut log = None;
        let mut matched = None;
        match decision {
            Decision::Allow => {}
            Decision::Autosage { rule } => {
                kind = "autosage";
                matched = Some(rule);
            }
            Decision::Log { rule, comment } => {
                matched = Some(rule);
                log = Some((rule, comment));
            }
            Decision::Reject {
                rule,
                ban_days,
                quiet,
            } => {
                matched = Some(rule);
                if ban_days != 0 {
                    effects["bans"] = json!([{"days": ban_days, "reject": 1, "automatic": true, "reason": "banned", "filename_proxy": false}]);
                }
                if quiet {
                    kind = "quiet";
                } else {
                    kind = "error";
                    let error = if ban_days != 0 { "banned" } else { "rejected" };
                    message = Some(if case["test_board"].as_bool().unwrap() {
                        format!("{error} (filter ID: {rule})")
                    } else {
                        error.into()
                    });
                }
            }
            Decision::FilenameProxy => {
                kind = "error";
                message = Some("generic".into());
                effects["bans"] = json!([{"days": 14, "reject": 1, "automatic": false, "reason": "Proxy/Tor exit node.", "filename_proxy": true}]);
            }
            Decision::InvalidSubject {
                logged_rule,
                comment,
            } => {
                matched = logged_rule;
                log = logged_rule.map(|rule| (rule, comment));
                kind = "error";
                message = Some("You can't post with that subject.".into());
            }
        }
        if let Some(rule) = matched {
            effects["hits"] = json!([rule]);
        }
        if let Some((rule, comment)) = log {
            effects["logs"] = json!([{"id": rule, "board": input["board"], "reply": input["reply"],
                "name": input["name"], "subject": input["subject"], "comment": comment, "filename": input["filename"]}]);
        }
        assert_eq!(
            json!({"kind": kind, "message": message}),
            case["outcome"],
            "outcome {compared}: {}",
            case["label"]
        );
        assert_eq!(
            effects, case["effects"],
            "effects {compared}: {}",
            case["label"]
        );
        compared += 1;
    }
    assert_eq!(unavailable, 20);
    assert_eq!(compared + unavailable, cases.len());
    assert!(compared > 400);
}

#[test]
fn empty_regexp_count_retries_nonempty_at_the_same_offset() {
    for (text, expected) in [("", 1), ("a", 3), ("aa", 5), ("a a", 6), ("aé a", 7)] {
        let regex = BoundedRegex::compile("/|a/u").unwrap();
        assert!(
            regex
                .matches(text, expected, &mut MAX_MATCH_CALLS.clone())
                .unwrap()
        );
        assert!(
            !regex
                .matches(text, expected + 1, &mut MAX_MATCH_CALLS.clone())
                .unwrap()
        );
    }
}

#[test]
fn regex_flags_delimiters_backreferences_and_anchoring_are_active() {
    for (pattern, text, expected) in [
        ("/paper/i", "PAPER", true),
        ("#paper/fold#", "paper/fold", true),
        ("{(?:paper){2}}", "paperpaper", true),
        ("/(paper)\\1/", "paperpaper", true),
        ("/paper(?=fold)/", "paperfold", true),
        ("/^fold/m", "paper\nfold", true),
        ("/paper.fold/s", "paper\nfold", true),
        ("/paper . fold/x", "paper-fold", true),
        ("/paper/A", "paperfold", true),
        ("/paper/A", "foldpaper", false),
        ("/\\w+/u", "é", true),
        ("/é/u", "é", true),
        ("/(?<x>a)|(?<x>b)/J", "b", true),
        ("/(a)/n", "a", true),
    ] {
        let regex = BoundedRegex::compile(pattern).unwrap();
        assert_eq!(
            regex
                .matches(text, 1, &mut MAX_MATCH_CALLS.clone())
                .unwrap(),
            expected,
            "{pattern} {text}"
        );
    }
}

#[test]
fn resource_limits_reject_bounded_workloads_and_cannot_be_raised_by_rules() {
    let workload = format!("{}!", "a".repeat(100));
    for pattern in [
        "/(*NO_START_OPT)(a+)+$/",
        "/(*LIMIT_MATCH=999999999)(*NO_START_OPT)(a+)+$/",
    ] {
        let regex = BoundedRegex::compile(pattern).unwrap();
        assert_eq!(
            regex.matches(&workload, 1, &mut MAX_MATCH_CALLS.clone()),
            Err(AdmissionError::WorkLimit)
        );
    }
    let regex = BoundedRegex::compile("/a/").unwrap();
    assert_eq!(
        regex.matches("a", 1, &mut 0),
        Err(AdmissionError::WorkLimit)
    );
    assert!(matches!(
        Policy::compile(vec![ordinary(&"a".repeat(MAX_PATTERN_BYTES + 1), false)]),
        Err(AdmissionError::InvalidPolicy)
    ));
    assert!(matches!(
        Policy::compile(
            (0..=MAX_RULES)
                .map(|id| {
                    let mut rule = ordinary("paper", false);
                    rule.id = id as i64 + 1;
                    rule
                })
                .collect()
        ),
        Err(AdmissionError::InvalidPolicy)
    ));
    assert!(matches!(
        Policy::compile(vec![ordinary("/incomplete", true)]),
        Err(AdmissionError::InvalidPolicy)
    ));
    assert!(matches!(
        Policy::compile(vec![ordinary("/paper/e", true)]),
        Err(AdmissionError::InvalidPolicy)
    ));
}

#[test]
fn actual_session_state_controls_leniency_threshold() {
    let now = 200_000;
    let mut state = crate::anonymous_session::State::new(now - 86_400);
    let mut rule = ordinary("paper", false);
    rule.lenient = true;
    let policy = Policy::compile(vec![rule]).unwrap();
    state.posts = 10;
    assert!(matches!(
        policy
            .evaluate(post("paper"), Actor::from_state(&state, now))
            .unwrap(),
        Decision::Reject { .. }
    ));
    state.posts = 11;
    assert_eq!(
        policy
            .evaluate(post("paper"), Actor::from_state(&state, now))
            .unwrap(),
        Decision::Allow
    );
    state.network_at = now;
    state.address_at = now;
    state.created_at = now;
    assert!(matches!(
        policy
            .evaluate(post("paper"), Actor::from_state(&state, now))
            .unwrap(),
        Decision::Reject { .. }
    ));
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn bounded_unicode_projection_and_fixed_policy_do_not_panic(input in ".{0,256}") {
        let policy = Policy::compile(vec![ordinary("paper", false)]).unwrap();
        let result = policy.evaluate(post(&input), Actor::default());
        proptest::prop_assert!(result.is_ok());
    }
}
