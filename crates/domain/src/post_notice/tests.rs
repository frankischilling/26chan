use super::*;
#[test]
fn notice_selection_and_auto_clear_gates_match_executed_source() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/post-notice.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1440);
    for (index, case) in cases.iter().enumerate() {
        let enabled = case["enabled"].as_bool().unwrap();
        let notice = select(
            enabled,
            case["warning"].as_bool().unwrap(),
            case["manager"].as_bool().unwrap() || case["flag"].as_bool().unwrap(),
            case["custom"].as_str(),
        )
        .unwrap();
        assert_eq!(
            notice.as_ref().map(Notice::source_html).as_deref(),
            case["html"].as_str(),
            "case {index}"
        );
        assert_eq!(
            enabled && clears_reports(case["action"].as_str().unwrap()),
            case["clear"].as_bool().unwrap(),
            "case {index}"
        );
        assert_eq!(
            if enabled { 2 } else { 0 },
            case["writes"].as_i64().unwrap()
        );
    }
}
#[test]
fn disabled_and_unprivileged_custom_fields_do_not_gain_notice_authority() {
    let oversized = "x".repeat(MAX_NOTICE_BYTES + 1);
    assert_eq!(select(false, false, true, Some(&oversized)).unwrap(), None);
    assert_eq!(
        select(true, false, false, Some(&oversized))
            .unwrap()
            .unwrap()
            .text(),
        BANNED
    );
    assert!(select(true, false, true, Some(&oversized)).is_err());
    assert!(Notice::from_saved("").is_err());
    assert!(Notice::from_saved("a\0b").is_err());
    assert_eq!(
        select(true, true, true, Some("0")).unwrap().unwrap().text(),
        "0"
    );
    assert_eq!(
        select(true, true, true, Some("")).unwrap().unwrap().text(),
        WARNED
    );
}
