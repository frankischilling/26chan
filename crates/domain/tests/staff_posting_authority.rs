use board_domain::{capcode::Capcode, robot9000};

#[test]
fn robot9000_authority_matches_all_pinned_source_predicate_cases() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/staff-posting-authority.json")).unwrap();
    let cases = fixture["robot_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 224);
    for case in cases {
        let label = case["capcode"].as_str().unwrap();
        let capcode = if label == "none" {
            None
        } else {
            Some(Capcode::parse(label).unwrap())
        };
        assert_eq!(
            robot9000::applies_to_post(
                case["enabled"].as_bool().unwrap(),
                capcode,
                case["options_field"].as_str().unwrap(),
                case["janitor_or_higher"].as_bool().unwrap(),
            ),
            case["applies"].as_bool().unwrap(),
            "{case}"
        );
    }
}
