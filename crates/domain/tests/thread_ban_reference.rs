//! Source PHP casts captured independently beside unchanged cpban execution.
use serde_json::Value;

#[test]
fn thread_days_match_executed_php_string_integer_coercion() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/thread-ban-source.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for (field, output) in [("op_days", "op"), ("rep_days", "rep")] {
            let raw = case[field]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| case[field].as_i64().unwrap().to_string());
            assert_eq!(
                board_domain::php_scalar::integer(&raw),
                case["observed"]["parsed_days"][output].as_i64().unwrap(),
                "{}: {}",
                case["name"],
                field
            );
        }
    }
}
