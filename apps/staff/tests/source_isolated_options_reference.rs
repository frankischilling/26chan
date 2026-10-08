//! Audit-only source contract. Sparse ReportQueue examples are not isolated controls.

fn mask(flags: &serde_json::Value) -> usize {
    ["sticky", "permasage", "closed", "permaage", "undead"]
        .into_iter()
        .enumerate()
        .fold(0, |mask, (bit, name)| {
            mask | (usize::from(flags[name].as_bool().unwrap()) << bit)
        })
}

fn check_audit(row: &serde_json::Value) {
    let old = mask(&row["old"]);
    let new = mask(&row["prepared"]);
    assert_eq!(row["logged"], old != new);
    if old == new {
        assert!(row["audit"].is_null());
    } else {
        assert_eq!(row["audit"]["old_mask"], old);
        assert_eq!(row["audit"]["new_mask"], new);
    }
}

#[test]
fn isolated_option_audits_change_only_the_requested_bit() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/staff-isolated-options.json")).unwrap();
    assert_eq!(
        fixture["selected_sha256"]["audit_helper"],
        "08e17fbdcbab63049e2ac503d23c704c52f00099174cfab461fe4471ec9ccf3a"
    );
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 128);
    let mut seen = [[false; 4]; 32];
    for row in rows {
        let old = mask(&row["old"]);
        let (index, bit, requested) = match row["action"].as_str().unwrap() {
            "close" => (0, 4, true),
            "reopen" => (1, 4, false),
            "permasage" => (2, 2, true),
            "unpermasage" => (3, 2, false),
            other => panic!("Unexpected action {other}"),
        };
        assert!(!seen[old][index]);
        seen[old][index] = true;
        assert_eq!(
            mask(&row["prepared"]),
            if requested { old | bit } else { old & !bit }
        );
        check_audit(row);
    }
    assert!(seen.into_iter().flatten().all(|present| present));
    let sparse = fixture["sparse_reportqueue_examples"].as_array().unwrap();
    assert_eq!(sparse.len(), 4);
    for (row, expected) in sparse.iter().zip([(2, 2), (23, 2), (31, 10), (31, 2)]) {
        assert_eq!((mask(&row["old"]), mask(&row["prepared"])), expected);
        assert!(row["old"]["permasage"].as_bool().unwrap());
        assert!(row["prepared"]["permasage"].as_bool().unwrap());
        check_audit(row);
    }
}
