use board_domain::drawing_annotation::{
    AnnotationDisplay, AnnotationGate, AnnotationInput, AnnotationLink, DrawingTime,
    MAX_DRAWING_SECONDS, SourcePost, SourceTarget, eligible_source_post_id, project_annotation,
};
use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use std::num::NonZeroU64;

const FROZEN: &[(&str, &str, &str)] = &[
    (
        "cases",
        include_str!("fixtures/drawing-annotation/cases.json"),
        "f80dc45486de96beb68b0ad1441e9d46b3dbc29c5895fa6b3409b2a1c5a9b589",
    ),
    (
        "time",
        include_str!("fixtures/drawing-annotation/time.json"),
        "56e724aed6ee6110a6377fe7e655f563e05b4e9ada1b37b82e8e797ca7f89e07",
    ),
    (
        "source-ids",
        include_str!("fixtures/drawing-annotation/source-ids.json"),
        "406ce5ec2ac22b409b3f5afd09771a0a3f471e3e13031da491fa5f50315c1646",
    ),
    (
        "posting-gates",
        include_str!("fixtures/drawing-annotation/posting-gates.json"),
        "43e30a6b6357d7d77d2dcd89a71b53fd0e8acdc39b6058b3e94549d09002cad0",
    ),
    (
        "source-pins",
        include_str!("fixtures/drawing-annotation/source-pins.json"),
        "f9cef0504f29571f97a06e4b66f906754d68718132dbf206f8b9d2664419f3c8",
    ),
    (
        "runtime",
        include_str!("fixtures/drawing-annotation/runtime.json"),
        "b420cefcd17e696e653a76de90eb35c65d25625f066828e14068ddaee91fd31c",
    ),
];

const ENABLED: AnnotationGate = AnnotationGate {
    accepted_image: true,
    painter_enabled: true,
    replays_enabled: true,
};

fn fixture(name: &str) -> Value {
    let (_, bytes, _) = FROZEN.iter().find(|row| row.0 == name).unwrap();
    serde_json::from_str(bytes).unwrap()
}

fn integer(value: &Value) -> i64 {
    value.as_i64().expect("parsed integer fixture input")
}

fn optional_positive(value: &Value) -> Option<NonZeroU64> {
    match value {
        Value::Null => None,
        value => {
            let value = integer(value);
            assert!(value >= 0, "fixture identifier must be nonnegative");
            NonZeroU64::new(value as u64)
        }
    }
}

/// Test-only projection of original frozen PHP markup. Production returns no
/// markup, and this intentionally does not test a future renderer or URL.
fn assert_original_display(name: &str, actual: Option<AnnotationDisplay>, html: &str) {
    if html.is_empty() {
        assert_eq!(actual, None, "{name}");
        return;
    }
    let annotation = html
        .strip_prefix("<br><br><small><b>Oekaki Post</b> (Time: ")
        .and_then(|s| s.strip_suffix(")</small>"))
        .expect("exact frozen source annotation wrapper");
    let (time, link) = match annotation.split_once(", ") {
        None => (annotation, None),
        Some((time, tail)) => {
            let link = if let Some(source) = tail.strip_prefix("Source: &gt;&gt;") {
                AnnotationLink::SourcePost(NonZeroU64::new(source.parse().unwrap()).unwrap())
            } else {
                let replay = tail
                    .strip_prefix("Replay: <a href=\"javascript:oeReplay(")
                    .and_then(|s| s.strip_suffix(");\">View</a>"))
                    .expect("exact frozen legacy replay wrapper");
                assert!(replay.parse::<u64>().unwrap() > 0, "{name}");
                AnnotationLink::Replay
            };
            (time, Some(link))
        }
    };
    let actual = actual.unwrap_or_else(|| panic!("missing annotation: {name}"));
    assert_eq!(actual.time.to_string(), time, "{name}");
    assert_eq!(actual.link, link, "{name}");
}

#[test]
fn independent_frozen_outcomes_and_source_provenance_are_pinned() {
    for (name, bytes, expected) in FROZEN {
        let actual = digest(&SHA256, bytes.as_bytes())
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(&actual, expected, "{name}: frozen oracle bytes changed");
    }
    let pins = fixture("source-pins");
    assert_eq!(
        pins["files"]["lib/oekaki.php"],
        json!({"bytes": 3619, "sha256":
            "44c1cc0f4e3553b7091019143106e9a4b124ba8dd2d88bd444fed7a9d2e4f3bf"})
    );
    assert_eq!(
        pins["files"]["imgboard.php"],
        json!({"bytes": 296536, "sha256":
            "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"})
    );
    for (name, lines, expected) in [
        (
            "php_source_id",
            [102, 124],
            "37e4a4ef071bc73d5ad477723399f53fc5c6981e996535c12672c4355eb2e251",
        ),
        (
            "php_time_annotation",
            [126, 160],
            "577dd0c760119e3bf3a935535b0e14144c4a82da1d55732a58439b812871cfcf",
        ),
        (
            "php_oekaki_upload_gate",
            [6063, 6097],
            "b2e854be3b1e15ca78e241a90186b96f48fd8e1eb16df6c661a01066850990eb",
        ),
    ] {
        let snippet = pins["snippets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        assert_eq!(snippet["start_line"], lines[0]);
        assert_eq!(snippet["end_line"], lines[1]);
        assert_eq!(snippet["sha256"], expected);
    }
    assert_eq!(fixture("runtime")["php"]["php"], "8.3.6");
    assert_eq!(fixture("runtime")["php"]["pcre"], "10.42 2022-12-11");
}

#[test]
fn parsed_time_and_link_intents_match_original_php_outcomes() {
    let time = fixture("time");
    let mut excluded = Vec::new();
    let mut compared = 0;
    for case in time.as_array().unwrap() {
        let name = case["id"].as_str().unwrap();
        let args = case["input"].as_array().unwrap();
        if !args[0].is_i64()
            || args[1..]
                .iter()
                .any(|value| !value.is_null() && !value.is_i64())
        {
            excluded.push(name);
            continue;
        }
        assert_eq!(case["result"]["outcome"], "return", "{name}");
        assert_eq!(case["result"]["warnings"], json!([]), "{name}");
        let actual = project_annotation(AnnotationInput {
            gate: ENABLED,
            wall_clock_seconds: Some(integer(&args[0])),
            has_stored_replay: optional_positive(&args[1]).is_some(),
            resolved_source_post_id: optional_positive(&args[2]),
        });
        assert_original_display(name, actual, case["result"]["value"].as_str().unwrap());
        compared += 1;
    }
    assert_eq!(compared, 29);
    // These are raw PHP coercion/truthiness probes, not the domain API. Do not
    // silently translate them or label this typed projection PHP-compatible.
    assert_eq!(
        excluded,
        [
            "numeric_string",
            "fraction_truncated",
            "nonnumeric",
            "null",
            "leading_numeric_string",
            "truthy_source_cast_after_branch",
            "truthy_replay_cast_after_branch",
            "zero_string_source",
        ]
    );
}

fn supplied_post(tables: &Value, requested_id: i64) -> Option<SourcePost<'_>> {
    // Search both synthetic boards so the helper, not this test lookup, must
    // reject a supplied record from another board. This executes no SQL.
    tables
        .as_object()
        .unwrap()
        .iter()
        .find_map(|(board, rows)| {
            rows.as_array().unwrap().iter().find_map(|row| {
                (integer(&row[0]) == requested_id).then(|| SourcePost {
                    board,
                    post_id: integer(&row[0]),
                    parent_thread_id: integer(&row[1]),
                    image_tim: integer(&row[2]),
                    // row[3] is filedeleted, intentionally not part of eligibility.
                })
            })
        })
}

#[test]
fn supplied_record_eligibility_matches_frozen_source_id_results() {
    let cases = fixture("cases");
    let source = fixture("source-ids");
    let mut excluded = Vec::new();
    let mut compared = 0;
    assert_eq!(source["tables"], cases["tables"]);
    for case in source["results"].as_array().unwrap() {
        let name = case["id"].as_str().unwrap();
        let args = case["input"].as_array().unwrap();
        if !args[0].is_i64() || !args[2].is_i64() {
            excluded.push(name);
            continue;
        }
        let source_id = integer(&args[0]);
        let result = eligible_source_post_id(
            SourceTarget {
                board: args[1].as_str().unwrap(),
                thread_id: integer(&args[2]),
            },
            source_id,
            supplied_post(&cases["tables"], source_id),
        );
        assert_eq!(case["result"]["outcome"], "return", "{name}");
        assert_eq!(
            result,
            optional_positive(&case["result"]["value"]),
            "{name}"
        );
        compared += 1;
    }
    assert_eq!(compared, 16);
    assert_eq!(
        excluded,
        [
            "source_numeric_string",
            "source_truncation",
            "source_nonnumeric",
            "thread_nonnumeric",
        ]
    );
    // Query failure/row-count handling remains a future store obligation.
    assert_eq!(source["row_counts"].as_array().unwrap().len(), 3);
}

#[test]
fn post_upload_annotation_decisions_match_frozen_posting_outcomes() {
    let cases = fixture("cases");
    let gates = fixture("posting-gates");
    let mut excluded = Vec::new();
    let mut compared = 0;
    for oracle in gates.as_array().unwrap() {
        let name = oracle["id"].as_str().unwrap();
        let case = cases["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == name)
            .unwrap();
        let time = &case["post"]["oe_time"];
        if oracle["result"]["outcome"] != "return" || (!time.is_null() && !time.is_i64()) {
            excluded.push(name);
            continue;
        }
        let gate = AnnotationGate {
            accepted_image: case["has_image"].as_bool().unwrap_or(true),
            painter_enabled: case["painter_enabled"].as_bool().unwrap_or(true),
            replays_enabled: case["replays_enabled"].as_bool().unwrap_or(true),
        };
        let thread_id = case["resto"].as_i64().unwrap_or(100);
        let source_id = case["post"]["oe_src"].as_i64();
        let resolve_source =
            gate.source_resolution_requested(!time.is_null(), source_id.is_some(), thread_id);
        let did_query = oracle["result"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call[0] == "mysql_board_call");
        // All source IDs/thread IDs in this frozen posting subset are positive
        // when resolution is attempted, so the function call reaches its query.
        assert_eq!(resolve_source, did_query, "{name}");
        let resolved = if resolve_source {
            let source_id = source_id.unwrap();
            eligible_source_post_id(
                SourceTarget {
                    board: "a",
                    thread_id,
                },
                source_id,
                supplied_post(&cases["tables"], source_id),
            )
        } else {
            None
        };
        assert_eq!(
            resolved,
            optional_positive(&oracle["result"]["value"]["source_id"]),
            "{name}"
        );
        // Replay storage is observed input, not something this API validates.
        let has_stored_replay = !oracle["result"]["value"]["replay_path"].is_null();
        let input = AnnotationInput {
            gate,
            wall_clock_seconds: time.as_i64(),
            has_stored_replay,
            resolved_source_post_id: resolved,
        };
        let original_annotation = oracle["result"]["value"]["comment"]
            .as_str()
            .unwrap()
            .strip_prefix("Synthetic comment")
            .unwrap();
        assert_original_display(name, project_annotation(input), original_annotation);
        assert_eq!(input.has_stored_replay, has_stored_replay, "{name}");
        compared += 1;
    }
    assert_eq!(compared, 16);
    assert_eq!(
        excluded,
        [
            "malformed_replay_hard_error",
            "upload_error_hard_error",
            "replay_move_failure",
            "fractional_time_annotation",
        ]
    );
}

#[test]
fn time_range_checks_are_total_and_preserve_rounding_quirks() {
    for seconds in [i64::MIN, -1, 0, MAX_DRAWING_SECONDS + 1, i64::MAX] {
        assert_eq!(DrawingTime::from_seconds(seconds), None);
    }
    for (seconds, expected) in [
        (1, "1s"),
        (59, "59s"),
        (60, "1m"),
        (89, "1m"),
        (90, "2m"),
        (3569, "59m"),
        (3570, "60m"),
        (3599, "60m"),
        (3600, "1h 0m"),
        (7169, "1h 59m"),
        (7170, "1h 60m"),
        (7199, "1h 60m"),
        (7200, "2h 0m"),
        (5_183_999, "1439h 60m"),
        (5_184_000, "1440h 0m"),
    ] {
        let time = DrawingTime::from_seconds(seconds).unwrap();
        assert_eq!(time.seconds(), seconds as u32);
        assert_eq!(time.to_string(), expected);
    }
}

#[test]
fn source_resolution_uses_presence_even_when_annotation_time_is_invalid() {
    for seconds in [i64::MIN, 0, MAX_DRAWING_SECONDS + 1, i64::MAX] {
        let time = Some(seconds);
        assert!(ENABLED.source_resolution_requested(time.is_some(), true, 100));
        assert_eq!(
            project_annotation(AnnotationInput {
                gate: ENABLED,
                wall_clock_seconds: time,
                has_stored_replay: true,
                resolved_source_post_id: NonZeroU64::new(200),
            }),
            None
        );
    }
    assert!(!ENABLED.source_resolution_requested(false, true, 100));
    assert!(!ENABLED.source_resolution_requested(true, false, 100));
    assert!(!ENABLED.source_resolution_requested(true, true, 0));
    assert!(ENABLED.source_resolution_requested(true, true, -1));
    assert_eq!(
        eligible_source_post_id(
            SourceTarget {
                board: "a",
                thread_id: -1
            },
            200,
            Some(SourcePost {
                board: "a",
                post_id: 200,
                parent_thread_id: 0,
                image_tim: 1,
            })
        ),
        None
    );
}

#[test]
fn each_outer_gate_is_required_even_with_time_source_and_stored_replay() {
    for accepted_image in [false, true] {
        for painter_enabled in [false, true] {
            for replays_enabled in [false, true] {
                let gate = AnnotationGate {
                    accepted_image,
                    painter_enabled,
                    replays_enabled,
                };
                let enabled = accepted_image && painter_enabled && replays_enabled;
                assert_eq!(gate.source_resolution_requested(true, true, 100), enabled);
                for has_stored_replay in [false, true] {
                    for resolved_source_post_id in [None, NonZeroU64::new(200)] {
                        let input = AnnotationInput {
                            gate,
                            wall_clock_seconds: Some(90),
                            has_stored_replay,
                            resolved_source_post_id,
                        };
                        assert_eq!(project_annotation(input).is_some(), enabled);
                        assert_eq!(
                            project_annotation(AnnotationInput {
                                wall_clock_seconds: None,
                                ..input
                            }),
                            None
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_supplied_different_post_never_satisfies_the_requested_source_id() {
    assert_eq!(
        eligible_source_post_id(
            SourceTarget {
                board: "a",
                thread_id: 100
            },
            101,
            Some(SourcePost {
                board: "a",
                post_id: 200,
                parent_thread_id: 0,
                image_tim: 1,
            })
        ),
        None
    );
}
