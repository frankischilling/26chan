//! Source request file-policy decisions, separate from storage authority.
//!
//! A source copy request is neither proof that a file exists nor proof that it
//! was copied. Native custody must bind these decisions to approved media and
//! commit them atomically with request creation/review. No file paths or private
//! digests are accepted from a browser.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention {
    None,
    JsonOnly,
    Everything,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blacklist {
    None,
    Image,
    RejectImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileRule {
    /// The source inserts a global digest rule with an indefinite ban.
    Ban,
    Reject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capture {
    /// Both source and thumbnail are attempted, even if a source copy fails.
    /// The native storage adapter must independently require approved media.
    pub copy_requested: bool,
    /// The source exposes the raw digest in captured JSON only in this branch.
    pub exposes_digest: bool,
}

pub fn capture(policy: Retention, illegal_report: bool) -> Capture {
    let copy_requested = policy == Retention::Everything && !illegal_report;
    Capture {
        copy_requested,
        exposes_digest: !copy_requested,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Review {
    pub captured: Capture,
    pub amended: bool,
    pub accepted_retention: Retention,
    /// Use the review caller's warning branch, not the stored zero-day duration.
    pub warning_action: bool,
    pub blacklist: Blacklist,
    pub has_extension: bool,
    /// Known source evidence. Unknown native evidence must not become false.
    pub digest_present: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    pub remove_captured_files: bool,
    pub exposes_digest: bool,
    pub blacklist: Option<FileRule>,
}

pub fn review(input: Review) -> Decision {
    let remove_captured_files =
        input.amended && input.has_extension && input.accepted_retention == Retention::JsonOnly;
    let exposes_digest = input.captured.exposes_digest || remove_captured_files;
    let blacklist = if input.warning_action || !exposes_digest || !input.digest_present {
        None
    } else {
        match input.blacklist {
            Blacklist::None => None,
            Blacklist::Image => Some(FileRule::Ban),
            Blacklist::RejectImage => Some(FileRule::Reject),
        }
    };
    Decision {
        remove_captured_files,
        exposes_digest,
        blacklist,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Clear {
    pub remove_source: bool,
    pub remove_thumbnail: bool,
}

pub fn clear(approved: bool) -> Clear {
    Clear {
        remove_source: true,
        remove_thumbnail: !approved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn retention(value: &Value) -> Retention {
        match value.as_str().unwrap() {
            "none" => Retention::None,
            "json_only" => Retention::JsonOnly,
            "everything" => Retention::Everything,
            _ => panic!("Unqualified fixture policy"),
        }
    }

    fn flag(value: &Value, key: &str) -> bool {
        value[key].as_bool().unwrap()
    }

    #[test]
    fn decisions_match_executed_source_file_branches() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/janitor-file-effects.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1728);
        for case in cases {
            let input = &case["input"];
            let expected = &case["expected"];
            let captured = capture(retention(&input["capture_policy"]), flag(input, "illegal"));
            let blacklist = match input["blacklist"].as_str().unwrap() {
                "none" => Blacklist::None,
                "image" => Blacklist::Image,
                "rejectimage" => Blacklist::RejectImage,
                _ => panic!("Unqualified blacklist policy"),
            };
            let decision = review(Review {
                captured,
                amended: flag(input, "amended"),
                accepted_retention: retention(&input["review_policy"]),
                warning_action: flag(input, "warning_action"),
                blacklist,
                has_extension: flag(input, "file"),
                digest_present: flag(input, "digest_present"),
            });
            assert_eq!(
                u64::from(captured.copy_requested) * 2,
                expected["copy_count"],
                "{input}"
            );
            assert_eq!(captured.exposes_digest, expected["capture_raw"], "{input}");
            assert_eq!(decision.exposes_digest, expected["review_raw"], "{input}");
            assert_eq!(
                u64::from(decision.remove_captured_files) * 2,
                expected["unlink_count"],
                "{input}"
            );
            assert_eq!(
                u64::from(decision.blacklist.is_some()),
                expected["blacklist_count"],
                "{input}"
            );
            assert_eq!(
                decision.blacklist.map(|mode| mode == FileRule::Ban),
                expected["blacklist_ban"].as_bool(),
                "{input}"
            );
            assert_eq!(
                decision.remove_captured_files, expected["undefined_approved"],
                "{input}"
            );
            assert_eq!(
                !flag(input, "warning_action")
                    && blacklist != Blacklist::None
                    && !decision.exposes_digest,
                expected["undefined_digest"],
                "{input}"
            );
            assert_eq!(expected["report_target"], serde_json::json!(["g", 7]));
        }
        for case in fixture["clear"].as_array().unwrap() {
            let effects = clear(flag(case, "approved"));
            assert!(effects.remove_source);
            assert_eq!(
                case["unlinks"].as_array().unwrap().len(),
                1 + usize::from(effects.remove_thumbnail)
            );
        }
    }
}
