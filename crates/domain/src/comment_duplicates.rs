//! Duplicate-comment admission at the source's pre-insert flood check.
//! Candidate selection must use protected same-board, same-peer history.

pub const WINDOW_SECONDS: i64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaffClass {
    Ordinary,
    Janitor,
    Moderator,
}

pub fn applies(quiet: bool, comment: &str, staff: StaffClass, meta: bool, name: &str) -> bool {
    !quiet
        && !comment.is_empty()
        && comment != "0"
        && match staff {
            StaffClass::Ordinary => true,
            StaffClass::Janitor => meta || !name.is_empty(),
            StaffClass::Moderator => false,
        }
}

/// The source uses a strict lower bound and no upper bound on candidate time.
/// A zero MAX(time) is false in PHP, even when it satisfies the lower bound.
pub fn remaining(now: i64, latest: Option<i64>) -> Option<i64> {
    let latest = latest?;
    if latest == 0 || latest <= now.checked_sub(WINDOW_SECONDS)? {
        return None;
    }
    latest.checked_add(WINDOW_SECONDS)?.checked_sub(now)
}

pub fn refusal(seconds: i64) -> String {
    let mut duration = String::new();
    let hours = seconds / 3600;
    let minutes = seconds / 60;
    if hours != 0 {
        duration.push_str(&format!(
            "{hours} hour{} ",
            if hours == 1 { "" } else { "s" }
        ));
    }
    if minutes != 0 {
        let minutes = minutes % 60;
        duration.push_str(&format!(
            "{minutes} minute{}",
            if minutes == 1 { "" } else { "s" }
        ));
    }
    let seconds = seconds % 60;
    if seconds != 0 {
        duration.push_str(&format!(
            " {seconds} second{}",
            if seconds == 1 { "" } else { "s" }
        ));
    }
    format!("Error: You must wait {duration} before posting a duplicate reply.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_executed_source_branch() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/comment-duplicates.json"))
                .unwrap();
        let cases = cases.as_array().unwrap();
        assert_eq!(cases.len(), 1260);
        for case in cases {
            let staff = match case["role"].as_str().unwrap() {
                "janitor" => StaffClass::Janitor,
                "moderator" => StaffClass::Moderator,
                "ordinary" | "vip" | "pass" => StaffClass::Ordinary,
                _ => panic!("Unknown source role"),
            };
            let actual = if applies(
                case["quiet"].as_bool().unwrap(),
                case["comment"].as_str().unwrap(),
                staff,
                case["meta"].as_bool().unwrap(),
                case["name"].as_str().unwrap(),
            ) {
                let latest = case["times"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|time| time.as_i64().unwrap())
                    .max();
                remaining(1000, latest).map(refusal)
            } else {
                None
            };
            assert_eq!(actual.as_deref(), case["result"].as_str(), "{case}");
        }
    }

    #[test]
    fn strict_boundary_and_future_candidates() {
        assert_eq!(remaining(1000, Some(700)), None);
        assert_eq!(remaining(1000, Some(701)), Some(1));
        assert_eq!(remaining(1000, Some(1000)), Some(300));
        assert_eq!(remaining(1000, Some(1040)), Some(340));
        assert_eq!(remaining(1, Some(0)), None);
        assert_eq!(remaining(1000, None), None);
    }

    #[test]
    fn source_truthiness_and_staff_exemptions() {
        for comment in ["", "0"] {
            assert!(!applies(false, comment, StaffClass::Ordinary, false, ""));
        }
        assert!(applies(false, "body", StaffClass::Ordinary, false, ""));
        assert!(!applies(true, "body", StaffClass::Ordinary, false, ""));
        assert!(!applies(false, "body", StaffClass::Janitor, false, ""));
        assert!(applies(false, "body", StaffClass::Janitor, false, "0"));
        assert!(applies(false, "body", StaffClass::Janitor, true, ""));
        assert!(!applies(false, "body", StaffClass::Moderator, true, "name"));
    }

    #[test]
    fn duration_preserves_source_spacing() {
        assert_eq!(
            refusal(1),
            "Error: You must wait  1 second before posting a duplicate reply."
        );
        assert_eq!(
            refusal(300),
            "Error: You must wait 5 minutes before posting a duplicate reply."
        );
        assert_eq!(
            refusal(3600),
            "Error: You must wait 1 hour 0 minutes before posting a duplicate reply."
        );
    }
}
