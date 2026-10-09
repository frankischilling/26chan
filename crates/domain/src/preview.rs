//! Source REPLIES_SHOWN policy shared by JSON and HTML board previews.
pub const MAX_PREVIEW_REPLIES: usize = 5;

/// Call with persisted validated board policy. Sticky threads use at most one
/// reply, while a board configured for OP-only previews stays OP-only.
pub fn reply_limit(configured: usize, sticky: bool) -> usize {
    if sticky {
        configured.min(1)
    } else {
        configured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sticky_preserves_zero_and_caps_other_source_configurations() {
        for configured in 0..=MAX_PREVIEW_REPLIES {
            assert_eq!(reply_limit(configured, false), configured);
            assert_eq!(reply_limit(configured, true), usize::from(configured > 0));
        }
    }
    #[test]
    fn pinned_source_fixture_sets_the_effective_preview_limit() {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/preview-policy-reference.json"
        ))
        .unwrap();
        let cases = reference["cases"].as_array().unwrap();
        assert!(cases.len() >= 172);
        for case in cases {
            let configured = case["configured_limit"].as_u64().unwrap() as usize;
            assert!(configured <= MAX_PREVIEW_REPLIES);
            assert_eq!(
                reply_limit(configured, case["sticky"].as_bool().unwrap()),
                case["expected"]["effective_limit"].as_u64().unwrap() as usize,
                "{}",
                case["id"]
            );
        }
    }
}
