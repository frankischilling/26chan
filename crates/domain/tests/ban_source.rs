use board_domain::ban::{Disposition, MatchKey, MatchedRestriction, QualifiedContext, decide};

#[test]
fn matches_executed_original_posting_restrictions() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/ban-enforcement.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13_840);
    for case in cases {
        let mut matches = Vec::new();
        for key in case["field_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .chain(std::iter::once("host"))
        {
            if key == "4pass_id" && !case["pass_present"].as_bool().unwrap() {
                continue;
            }
            for row in case["bans"].as_array().unwrap() {
                if !row["active"].as_bool().unwrap() || row["key"].as_str().unwrap() != key {
                    continue;
                }
                let end = row["ends_on"].as_i64().unwrap();
                matches.push(MatchedRestriction {
                    id: row["no"].as_i64().unwrap(),
                    key: match key {
                        "host" => MatchKey::Address,
                        "password" => MatchKey::DeletionCredential,
                        "4pass_id" => MatchKey::Pass,
                        _ => unreachable!(),
                    },
                    global: row["global"].as_i64().unwrap() != 0,
                    board: row["board"].as_str().unwrap(),
                    starts_at: row["starts_on"].as_i64().unwrap(),
                    ends_at: (end != 0).then_some(end),
                    template_id: row["template_id"].as_i64().unwrap(),
                    automatic: row["admin"] == "Auto-ban",
                    reason: row["reason"].as_str().unwrap(),
                });
            }
        }
        let result = decide(
            QualifiedContext {
                board: "test",
                request_at: 1_700_000_000,
                pass_present: case["pass_present"].as_bool().unwrap(),
                captcha_bypass: case["bypass"].as_bool().unwrap(),
                verified_user: case["verified"].as_bool().unwrap(),
            },
            &matches,
        );
        let expected = match case["decision"].as_i64().unwrap() {
            0 => Disposition::Allowed,
            1 => Disposition::Banned,
            2 => Disposition::Warned,
            _ => unreachable!(),
        };
        assert_eq!(result.disposition, expected, "{}", case["name"]);
        let expired: Vec<i64> = case["expired"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap())
            .collect();
        assert_eq!(result.expired, expired, "{}", case["name"]);
    }
}
