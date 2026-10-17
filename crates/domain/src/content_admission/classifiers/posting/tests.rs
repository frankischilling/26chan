use super::*;
struct Tape {
    digits: [u8; 2],
    coin: u8,
    digit_at: usize,
    coin_used: bool,
    ranges: Vec<u8>,
}
impl Tape {
    fn new(first: u8, coin: u8) -> Self {
        Self {
            digits: [first, 9],
            coin,
            digit_at: 0,
            coin_used: false,
            ranges: vec![],
        }
    }
}
impl Random for Tape {
    fn draw(&mut self, upper_exclusive: u8) -> Option<u8> {
        self.ranges.push(upper_exclusive);
        match upper_exclusive {
            10 => {
                let value = self.digits.get(self.digit_at).copied();
                self.digit_at += 1;
                value
            }
            2 if !self.coin_used => {
                self.coin_used = true;
                Some(self.coin)
            }
            _ => None,
        }
    }
}
fn request<'a>(actor: Option<&'a State>) -> Request<'a> {
    Request {
        board: "a",
        reply: true,
        image: false,
        captcha_bypass: false,
        actor: A(actor),
        activity_at: 10_000_000,
        score: NumericScore(A(0.0)),
        thread: U,
        browser_id: U,
        country: U,
        timing_cookie: U,
    }
}
#[test]
fn ordered_prefix_matches_executed_source_cases() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/late-post.json")).unwrap();
    assert_eq!(
        fixture["source_sha256"],
        "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
    );
    assert_eq!(
        fixture["prefix_sha256"],
        "7259bc1e5c1688a0da6c651318be19b562ad97ee6b9ad64547935ddf9e6af8f5"
    );
    assert_eq!(
        fixture["actor_sha256"],
        "0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26"
    );
    assert_eq!(fixture["php_version"], "8.4.24");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13280);
    let now = fixture["now"].as_u64().unwrap();
    let mut events = std::collections::BTreeSet::new();
    for (index, c) in cases.iter().enumerate() {
        let actor = (!c["actor"].is_null()).then(|| {
            let a = &c["actor"];
            State {
                created_at: now - a["age"].as_u64().unwrap(),
                network_at: now - a["network"].as_u64().unwrap(),
                posts: a["posts"].as_u64().unwrap().try_into().unwrap(),
                pending: a["pending"].as_u64().unwrap().try_into().unwrap(),
                verified_level: a["verified"].as_u64().unwrap().try_into().unwrap(),
                change_score: a["churn"].as_u64().unwrap().try_into().unwrap(),
                ..State::new(now)
            }
        });
        let input = Request {
            board: c["board"].as_str().unwrap(),
            reply: c["reply"].as_bool().unwrap(),
            image: c["image"].as_bool().unwrap(),
            captcha_bypass: c["bypass"].as_bool().unwrap(),
            actor: A(actor.as_ref()),
            activity_at: now,
            // A skipped source score must not require a provider in Rust.
            score: NumericScore(if c["score_calls"] == 0 {
                U
            } else {
                A(c["score"].as_f64().unwrap())
            }),
            thread: A(Thread {
                subject: c["subject"].as_str().unwrap().as_bytes(),
                comment: c["comment"].as_str().unwrap().as_bytes(),
            }),
            browser_id: A(c["browser"].as_str()),
            country: A(c["country"].as_str().unwrap().as_bytes()),
            timing_cookie: A(c["cookie"].as_str().map(str::as_bytes)),
        };
        let mut random = Tape::new(
            c["digit"].as_u64().unwrap().try_into().unwrap(),
            c["coin"].as_u64().unwrap().try_into().unwrap(),
        );
        let actual = evaluate(input, &mut random).unwrap();
        let outcome = match actual.outcome {
            Outcome::Continue => "continue",
            Outcome::Quiet => "quiet",
            Outcome::ImageRangeRejected => "image_range_rejected",
        };
        assert_eq!(outcome, c["outcome"].as_str().unwrap(), "case {index}");
        let actual_events: Vec<_> = actual
            .events
            .into_iter()
            .flatten()
            .map(Event::source_name)
            .collect();
        let expected_events: Vec<_> = c["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(actual_events, expected_events, "case {index}");
        events.extend(actual_events);
        let expected_ranges: Vec<u8> = c["draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap().try_into().unwrap())
            .collect();
        assert_eq!(random.ranges, expected_ranges, "case {index}");
    }
    assert_eq!(events.len(), 12, "Every active prefix event is exercised");
}
#[test]
fn unavailable_facts_are_needed_only_on_reached_branches() {
    let mut random = Tape::new(0, 0);
    let mut input = request(None);
    input.actor = U;
    input.score = NumericScore(U);
    assert_eq!(
        evaluate(input, &mut random),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(None);
    input.actor = U;
    input.score = NumericScore(U);
    input.captcha_bypass = true;
    assert_eq!(
        evaluate(input, &mut random).unwrap().outcome,
        Outcome::Continue
    );
    let known = State {
        created_at: 10_000_000 - 21600,
        network_at: 10_000_000 - 21600,
        ..State::new(10_000_000)
    };
    let mut input = request(Some(&known));
    input.score = NumericScore(U);
    assert_eq!(
        evaluate(input, &mut random).unwrap().outcome,
        Outcome::Continue
    );
    let mut input = request(None);
    input.score = NumericScore(U);
    assert_eq!(
        evaluate(input, &mut random),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(None);
    input.board = "vg";
    input.image = true;
    input.score = NumericScore(A(0.32));
    assert_eq!(
        evaluate(input, &mut random).unwrap().events[0],
        Some(Event::TextThreat)
    );
    let mut input = request(None);
    input.board = "vg";
    input.image = true;
    assert_eq!(
        evaluate(input, &mut random),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(None);
    input.board = "vg";
    input.image = true;
    input.thread = A(Thread {
        subject: b"/bag/",
        comment: b"",
    });
    assert_eq!(
        evaluate(input, &mut random),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(None);
    input.board = "vg";
    input.image = true;
    input.thread = A(Thread {
        subject: b"",
        comment: b"",
    });
    input.country = A(b"JP");
    assert_eq!(
        evaluate(input, &mut random).unwrap().outcome,
        Outcome::Continue
    );
    let mut input = request(None);
    input.board = "vg";
    input.image = true;
    input.thread = A(Thread {
        subject: b"",
        comment: b"",
    });
    input.country = A(b"US");
    assert_eq!(
        evaluate(input, &mut random),
        Err(AdmissionError::Unavailable)
    );
    assert!(random.ranges.is_empty());
}
#[test]
fn randomness_and_bounds_fail_closed_without_partial_events() {
    struct BadRandom;
    impl Random for BadRandom {
        fn draw(&mut self, limit: u8) -> Option<u8> {
            Some(limit)
        }
    }
    let mut input = request(None);
    input.board = "f";
    input.score = NumericScore(A(0.09));
    assert_eq!(
        evaluate(input, &mut BadRandom),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(None);
    input.score = NumericScore(A(f64::NAN));
    assert_eq!(
        evaluate(input, &mut BadRandom),
        Err(AdmissionError::InvalidPolicy)
    );
    let oversized = vec![b'x'; 16385];
    let mut input = request(None);
    input.board = "g";
    input.image = true;
    input.thread = A(Thread {
        subject: &oversized,
        comment: b"",
    });
    assert_eq!(
        evaluate(input, &mut BadRandom),
        Err(AdmissionError::InvalidPolicy)
    );
}
