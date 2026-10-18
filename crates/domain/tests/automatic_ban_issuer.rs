use board_domain::ban::{Disposition, MatchKey, MatchedRestriction, QualifiedContext, decide};

// This qualifies consumption of source-issued rows, not a native automatic-ban
// writer. Aligned timestamps model one explicit PHP/database timezone choice.
#[test]
fn source_issued_dates_preserve_warnings_before_expiry() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/automatic-ban-issuer-reference.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 420);
    for case in cases {
        let row = &case["insert"];
        if case["exempt"].as_bool().unwrap() {
            assert!(row.is_null());
            assert!(case["aligned_expiry"].is_null());
            continue;
        }
        assert_eq!(row["admin"], "Auto-ban");
        assert_eq!(row["reason"], "Public reason<>Auto-ban: Synthetic reason");
        assert_eq!(row["post_time"], "NOW()");
        assert!(row.get("active").is_none());
        assert!(row.get("now").is_none());
        let identity = case["identity"].as_bool().unwrap();
        assert_eq!(
            row["password"],
            if identity { "synthetic-password" } else { "" }
        );
        assert_eq!(
            row["4pass_id"],
            if identity { "synthetic-pass" } else { "" }
        );
        let start = case["clock"].as_i64().unwrap();
        let end = case["aligned_expiry"].as_i64();
        let days = case["days"].as_i64().unwrap();
        if days == -1 {
            assert_eq!(row["length"], "00000000000000");
            assert!(end.is_none());
        } else {
            assert!(row["length"].as_str().unwrap().ends_with("000000"));
            let expiry = end.unwrap();
            // Fixed-second addition followed by local midnight truncation.
            // The gap may span 23 or 25 hours near a clock change.
            assert!(expiry <= start + days.max(0) * 86_400);
            assert!(expiry > start + days.max(0) * 86_400 - 90_001);
        }
        let restriction = MatchedRestriction {
            id: 17,
            key: MatchKey::Address,
            global: true,
            board: "test",
            starts_at: start,
            ends_at: end,
            template_id: 0,
            automatic: true,
            reason: row["reason"].as_str().unwrap(),
        };
        for request_at in [start, start + 4 * 86_400]
            .into_iter()
            .chain(end.filter(|_| days > 0))
        {
            let result = decide(
                QualifiedContext {
                    board: "test",
                    request_at,
                    pass_present: false,
                    captcha_bypass: false,
                    verified_user: false,
                },
                &[restriction],
            );
            let expected = if days != -1 && days <= 0 {
                Disposition::Warned
            } else if days > 0 && end.is_some_and(|expiry| request_at >= expiry) {
                Disposition::Allowed
            } else {
                Disposition::Banned
            };
            assert_eq!(result.disposition, expected, "{case}");
            assert_eq!(
                result.expired,
                if expected == Disposition::Allowed {
                    vec![17]
                } else {
                    vec![]
                }
            );
        }
    }
}
