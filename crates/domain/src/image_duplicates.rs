//! Image-content duplicate gates, separate from actor posting cooldowns.
//! Badge eligibility and candidate facts must come from protected storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Duplicate,
    RepostLimit,
    RecentUpload,
}

pub const DUPLICATE: &str = "Error: Duplicate file exists.";
pub const RECENT_UPLOAD: &str = "Error: You must wait longer before reposting this file.";

pub fn decide(
    has_image: bool,
    badged: bool,
    reply: bool,
    scoped_duplicate: bool,
    repost_count: u32,
    repost_limit: u32,
    recent_upload: bool,
) -> Decision {
    if !has_image || badged {
        Decision::Allow
    } else if scoped_duplicate {
        Decision::Duplicate
    } else if reply && repost_limit > 0 && repost_count >= repost_limit {
        Decision::RepostLimit
    } else if recent_upload {
        Decision::RecentUpload
    } else {
        Decision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_order_matches_executed_source_profiles() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/image-duplicates.json")).unwrap();
        let cases = cases.as_array().unwrap();
        assert_eq!(cases.len(), 288);
        for case in cases {
            let reply = case["target"].as_i64().unwrap() > 0;
            let scenario = case["scenario"].as_str().unwrap();
            let profile = case["profile"].as_str().unwrap();
            let same = scenario == "op" || reply && scenario == "same_active";
            let count = match scenario {
                "count_below" => 2,
                "count_exact" => 3,
                "count_above" => 4,
                "same_active" | "other_thread" => 1,
                _ => 0,
            };
            let decision = decide(
                case["image"].as_bool().unwrap(),
                case["badge"] != "none",
                reply,
                same,
                count,
                if profile == "repost" { 3 } else { 0 },
                profile == "history" && scenario == "recent_history",
            );
            let actual = match decision {
                Decision::Allow => "allow",
                Decision::Duplicate => "duplicate",
                Decision::RepostLimit => "repost",
                Decision::RecentUpload => "recent",
            };
            assert_eq!(actual, case["result"].as_str().unwrap(), "{case}");
        }
    }

    #[test]
    fn earlier_duplicate_beats_repost_and_recent_history() {
        assert_eq!(
            decide(true, false, true, true, 3, 3, true),
            Decision::Duplicate
        );
        assert_eq!(
            decide(true, false, true, false, 3, 3, true),
            Decision::RepostLimit
        );
        assert_eq!(
            decide(true, false, true, false, 2, 3, true),
            Decision::RecentUpload
        );
        assert_eq!(decide(true, true, true, true, 3, 3, true), Decision::Allow);
    }
}
