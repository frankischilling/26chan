use super::*;
#[test]
fn ordered_history_matches_executed_source() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/post-history.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["source_sha256"],
        "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
    );
    assert_eq!(
        fixture["prefix_sha256"],
        "745b9b623820488740874d3fe401d019bb92ea48abb1e1cdbff967e3cec75677"
    );
    assert_eq!(
        fixture["actor_sha256"],
        "0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26"
    );
    assert_eq!(fixture["php_version"], "8.4.24");
    let now = fixture["now"].as_u64().unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12416);
    for (index, c) in cases.iter().enumerate() {
        let state = (!c["actor"].is_null()).then(|| {
            let a = &c["actor"];
            State {
                created_at: now - a["age"].as_u64().unwrap(),
                network_at: now - a["age"].as_u64().unwrap(),
                environment_at: now - u64::from(!a["changed"].as_bool().unwrap()),
                posts: a["posts"].as_u64().unwrap().try_into().unwrap(),
                pending: a["pending"].as_u64().unwrap().try_into().unwrap(),
                verified_level: a["verified"].as_u64().unwrap().try_into().unwrap(),
                ..State::new(now)
            }
        });
        // Any provider the PHP branch did not reach is unavailable in Rust.
        let fact = |key: &str, call: &str| {
            if c["calls"].as_array().unwrap().iter().any(|v| v == call) {
                A(c[key].as_bool().unwrap())
            } else {
                U
            }
        };
        let result = evaluate(Request {
            captcha_bypass: c["bypass"].as_bool().unwrap(),
            actor: A(state.as_ref()),
            activity_at: now,
            edge_present: A(!c["edge"].is_null()),
            edge_history: fact("edgeHistory", "edge_history"),
            automated: fact("automated", "automated"),
            country_history: fact("countryHistory", "country_history"),
        })
        .unwrap_or_else(|_| panic!("case {index}"));
        assert_eq!(
            result.rejected,
            c["rejected"].as_bool().unwrap(),
            "case {index}"
        );
        let events: Vec<_> = result
            .events
            .into_iter()
            .flatten()
            .map(|event| match event {
                Event::LogEdgeCooldown => "log:block_bm_bot_pwd",
                Event::RecordEdgeBlock => "record:block_bm_bot",
                Event::RecordCountryChange => "record:country_changed",
                Event::LogCountryBlock => "log:block_country_changed",
            })
            .collect();
        let expected: Vec<_> = c["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(events, expected, "case {index}");
    }
}
fn request(actor: Option<&State>) -> Request<'_> {
    Request {
        captcha_bypass: false,
        actor: A(actor),
        activity_at: 1000,
        edge_present: U,
        edge_history: U,
        automated: U,
        country_history: U,
    }
}
#[test]
fn missing_needed_history_does_not_become_a_negative_lookup() {
    let actor = State::new(900);
    assert_eq!(
        evaluate(request(Some(&actor))),
        Err(AdmissionError::Unavailable)
    );
    let mut input = request(Some(&actor));
    input.edge_present = A(false);
    assert_eq!(evaluate(input), Err(AdmissionError::Unavailable));
    let mut input = request(Some(&actor));
    input.edge_present = A(true);
    input.country_history = A(false);
    assert_eq!(evaluate(input), Err(AdmissionError::Unavailable));
    let mut input = request(Some(&actor));
    input.edge_present = A(true);
    input.edge_history = A(false);
    assert_eq!(evaluate(input), Err(AdmissionError::Unavailable));
    assert!(!evaluate(request(None)).unwrap().rejected);
    let mut input = request(None);
    input.actor = U;
    assert_eq!(evaluate(input), Err(AdmissionError::Unavailable));
    let mut input = request(None);
    input.actor = U;
    input.captcha_bypass = true;
    assert!(!evaluate(input).unwrap().rejected);
    let verified = State {
        verified_level: 1,
        ..actor
    };
    assert!(!evaluate(request(Some(&verified))).unwrap().rejected);
}
