//! Pure reference-contract check; database transition tests consume the same fixture.

#[test]
fn pinned_unsticky_assignment_and_audit_cover_all_unchanged_other_flags() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/staff-unsticky.json")).unwrap();
    assert_eq!(
        fixture["files"]["admin.php"],
        "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb"
    );
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 32);
    let mut seen = [false; 32];
    for row in rows {
        let old_mask = ["sticky", "permasage", "closed", "permaage", "undead"]
            .into_iter()
            .enumerate()
            .fold(0usize, |mask, (bit, name)| {
                mask | (usize::from(row[name].as_bool().unwrap()) << bit)
            });
        assert!(!seen[old_mask]);
        seen[old_mask] = true;
        assert_eq!(row["requested_sticky"], false);
        let was_sticky = old_mask & 1 != 0;
        let root = if was_sticky { "now()" } else { "root" };
        assert_eq!(row["root_expression"], root);
        assert_eq!(row["assignments"], format!("sticky=0,root={root},"));
        assert_eq!(row["logged"], was_sticky);
        if was_sticky {
            assert_eq!(row["audit"]["old_mask"], old_mask);
            assert_eq!(row["audit"]["new_mask"], old_mask & !1);
        } else {
            assert!(row["audit"].is_null());
        }
    }
    assert!(seen.into_iter().all(|present| present));
}
