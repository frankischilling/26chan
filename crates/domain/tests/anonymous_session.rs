use board_domain::anonymous_session::{Activity, Capability, Changes, State};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/anonymous-reference.json")).unwrap()
}

fn number(value: &Value, name: &str) -> u64 {
    value[name].as_u64().unwrap()
}

fn state(value: &Value) -> State {
    State {
        created_at: number(value, "creation_ts"),
        network_at: number(value, "mask_ts"),
        address_at: number(value, "ip_ts"),
        environment_at: number(value, "env_ts"),
        activity_at: number(value, "activity_ts"),
        action_at: number(value, "action_ts"),
        verified_level: number(value, "verified_level").try_into().unwrap(),
        posts: number(value, "post_count").try_into().unwrap(),
        images: number(value, "img_count").try_into().unwrap(),
        threads: number(value, "thread_count").try_into().unwrap(),
        reports: number(value, "report_count").try_into().unwrap(),
        pending: number(value, "action_buffer").try_into().unwrap(),
        change_score: number(value, "ip_change_score").try_into().unwrap(),
    }
}

#[test]
fn known_user_thresholds_match_the_pinned_source() {
    let reference = fixture();
    let now = number(&reference, "now");
    let cases = reference["known_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1370);
    for (index, case) in cases.iter().enumerate() {
        let mut session = State::new(now);
        session.created_at = now - number(case, "password_age");
        session.network_at = now - number(case, "network_age");
        session.posts = number(case, "posts").try_into().unwrap();
        session.reports = number(case, "reports").try_into().unwrap();
        session.pending = number(case, "pending").try_into().unwrap();
        session.change_score = number(case, "score").try_into().unwrap();
        session.verified_level = number(case, "verified").try_into().unwrap();
        let minutes = number(case, "minutes").try_into().unwrap();
        let since = number(case, "since");
        assert_eq!(
            session.is_known(now, minutes, since),
            case["known"].as_bool().unwrap(),
            "known-user case {index}: {case}"
        );
        assert_eq!(
            session.is_known_or_verified(now, minutes, since),
            case["known_or_verified"].as_bool().unwrap(),
            "verified case {index}: {case}"
        );
    }
}

#[test]
fn buffered_activity_and_churn_match_the_pinned_source() {
    let reference = fixture();
    let cases = reference["activity_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 966);
    for (index, case) in cases.iter().enumerate() {
        let mut session = state(&case["before"]);
        let kind = number(case, "kind");
        let activity = if kind == 8 {
            Activity::Report
        } else {
            assert_eq!(kind & !7, 0);
            Activity::Post {
                thread: kind & 4 != 0,
                image: kind & 2 != 0,
            }
        };
        session.update(
            number(case, "now"),
            activity,
            case["dummy"].as_bool().unwrap(),
        );
        assert_eq!(session, state(&case["after"]), "activity case {index}");
        let counts = [
            session.post_count(),
            session.image_count(),
            session.thread_count(),
            session.report_count(),
        ];
        for (count, expected) in counts.iter().zip(case["counts"].as_array().unwrap()) {
            assert_eq!(
                u64::from(*count),
                expected.as_u64().unwrap(),
                "case {index}"
            );
        }
    }
}

#[test]
fn idle_reset_matches_the_retained_password_branch() {
    let reference = fixture();
    let cases = reference["reset_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    for (index, case) in cases.iter().enumerate() {
        let mut session = state(&case["before"]);
        let expired = session.resume(number(case, "now"), Changes::default());
        assert_eq!(expired, case["expired"].as_bool().unwrap(), "reset {index}");
        assert_eq!(session, state(&case["after"]), "reset state {index}");
    }
}

#[test]
fn request_network_changes_preserve_age_and_verification_scope() {
    let now = 1_700_000_000;
    let mut session = State::new(now - 3600);
    session.activity_at = now - 1;
    session.posts = 3;
    session.verified_level = 1;
    assert!(!session.resume(
        now,
        Changes {
            network: true,
            address: false,
            environment: true,
        }
    ));
    assert_eq!(session.password_age(now), 3600);
    assert_eq!(session.network_age(now), 0);
    assert_eq!(session.address_age(now), 0);
    assert_eq!(session.environment_age(now), 0);
    assert_eq!(session.verified_level, 1);
    session.update(
        now,
        Activity::Post {
            thread: false,
            image: false,
        },
        false,
    );
    assert_eq!(session.change_score, 3);
    assert_eq!(session.post_count(), 4);
    assert!(!session.resume(
        now + 1,
        Changes {
            address: true,
            ..Changes::default()
        }
    ));
    assert_eq!(session.network_age(now + 1), 1);
    assert_eq!(session.address_age(now + 1), 0);
}

#[test]
fn clock_skew_cannot_create_age_or_overflow_policy_thresholds() {
    let now = 100;
    let session = State::new(now + 1);
    assert_eq!(session.password_age(now), 0);
    assert_eq!(session.network_age(now), 0);
    assert!(!session.is_known(now, u32::MAX, 0));
    assert!(session.is_known(now, 0, 0));
}

fn capability(byte: &str) -> Capability {
    Capability::parse(&format!("a1_{}", byte.repeat(32))).unwrap()
}

#[test]
fn capability_protocol_rejects_ambiguous_or_legacy_encodings() {
    let valid = format!("a1_{}", "ab".repeat(32));
    let parsed = Capability::parse(&valid).unwrap();
    assert_eq!(parsed.credential(), valid);
    for invalid in [
        String::new(),
        "a1_".into(),
        "a1_".repeat(100),
        format!("a1_{}", "ab".repeat(31)),
        format!("a1_{}", "ab".repeat(33)),
        format!("a1_{}", "AB".repeat(32)),
        format!("a1_{}", "gg".repeat(32)),
        format!("a2_{}", "ab".repeat(32)),
        format!(" {valid}"),
        format!("{valid}; other=1"),
        format!("{valid}\0"),
        "4chan_pass=legacy-cookie".into(),
    ] {
        assert!(Capability::parse(&invalid).is_none());
    }
    let first = Capability::generate().unwrap();
    let second = Capability::generate().unwrap();
    assert_ne!(first.storage_hash(), second.storage_hash());
    assert_eq!(
        Capability::parse(&first.credential())
            .unwrap()
            .storage_hash(),
        first.storage_hash()
    );
}

#[test]
fn network_hashes_have_the_source_ipv4_scope_and_a_separate_address_scope() {
    let token = capability("11");
    let first = token.fingerprints(Some("192.0.2.1".parse().unwrap()), *b"US");
    let nearby = token.fingerprints(Some("192.0.200.2".parse().unwrap()), *b"US");
    let changed = token.fingerprints(Some("192.1.2.1".parse().unwrap()), *b"US");
    let mapped = token.fingerprints(Some("::ffff:192.0.2.1".parse().unwrap()), *b"US");
    let country = token.fingerprints(Some("192.0.2.1".parse().unwrap()), *b"CA");
    assert_eq!(first.network, nearby.network);
    assert_ne!(first.address, nearby.address);
    assert_ne!(first.network, changed.network);
    assert!(first == mapped);
    assert_eq!(first.network, country.network);
    assert_eq!(first.address, country.address);
    assert_ne!(first.environment, country.environment);
    assert_ne!(first.network, first.address);
    assert_ne!(first.token, first.network);
    assert_ne!(first.environment, first.address);
}

#[test]
fn ipv6_networks_and_missing_transport_use_explicit_distinct_scopes() {
    let token = capability("11");
    let first = token.fingerprints(Some("2001:db8:1:2::1".parse().unwrap()), *b"XX");
    let nearby = token.fingerprints(Some("2001:db8:1:2::2".parse().unwrap()), *b"XX");
    let changed = token.fingerprints(Some("2001:db8:1:3::1".parse().unwrap()), *b"XX");
    let absent = token.fingerprints(None, *b"XX");
    let ipv4 = token.fingerprints(Some("0.0.0.0".parse().unwrap()), *b"XX");
    assert_eq!(first.network, nearby.network);
    assert_ne!(first.address, nearby.address);
    assert_ne!(first.network, changed.network);
    assert_ne!(absent.network, ipv4.network);
    assert_ne!(absent.address, ipv4.address);
}

#[test]
fn session_specific_hashes_cannot_be_shared_as_public_identity() {
    let first = capability("11").fingerprints(Some("192.0.2.1".parse().unwrap()), *b"US");
    let second = capability("22").fingerprints(Some("192.0.2.1".parse().unwrap()), *b"US");
    assert_ne!(first.token, second.token);
    assert_ne!(first.network, second.network);
    assert_ne!(first.address, second.address);
    assert_ne!(first.environment, second.environment);
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(128))]
    #[test]
    fn accepted_activity_remains_bounded_without_counter_wrap(
        events in proptest::collection::vec((0u16..=20_000, proptest::bool::ANY, proptest::bool::ANY, proptest::bool::ANY), 0..200)
    ) {
        let mut now = 1_700_000_000;
        let mut session = State::new(now);
        for (elapsed, report, thread, image) in events {
            now += u64::from(elapsed);
            session.resume(now, Changes::default());
            session.update(now, if report { Activity::Report } else { Activity::Post { thread, image } }, false);
            proptest::prop_assert!(session.pending <= 15);
            proptest::prop_assert!(session.change_score <= 32);
            proptest::prop_assert!(session.post_count() <= 256);
            proptest::prop_assert!(session.image_count() <= 256);
            proptest::prop_assert!(session.thread_count() <= 256);
            proptest::prop_assert!(session.report_count() <= 256);
            proptest::prop_assert!(session.activity_at <= now);
            proptest::prop_assert!(session.action_at <= now);
        }
    }
}
