//! Public VIP option syntax. Parsing grants no identity or posting authority.
//! Credential lookup and receipt consumption belong to the private verifier.

/// Deliberately not Debug or Serialize: the option contains a password.
pub struct Credentials<'a> {
    identifier: &'a str,
    password: &'a str,
}

impl<'a> Credentials<'a> {
    pub fn identifier(&self) -> &'a str {
        self.identifier
    }

    pub fn password(&self) -> &'a str {
        self.password
    }
}

/// Accepts the already bounded options after source ASCII sage removal.
/// `captcha_bypass` is a policy observation, never proof of authority.
pub fn credentials(prepared_options: &str, captcha_bypass: bool) -> Option<Credentials<'_>> {
    if !captcha_bypass
        || prepared_options.len() > crate::MAX_PUBLIC_FIELD_BYTES
        || !prepared_options.starts_with("capcode_!")
    {
        return None;
    }
    let mut parts = prepared_options.splitn(3, '!');
    parts.next()?;
    let identifier = parts.next()?;
    let password = parts.next()?;
    // PHP's string truthiness rejects exactly "" and "0", not all zero-like
    // strings. The third component retains any additional exclamation marks.
    if matches!(identifier, "" | "0") || matches!(password, "" | "0") {
        return None;
    }
    Some(Credentials {
        identifier,
        password,
    })
}

/// Source counts failures at or after the one-hour boundary, before parsing
/// the identifier/password. Storage must enforce this under its own locks.
pub fn failure_limit_reached(failures_in_hour: u64) -> bool {
    failures_in_hour >= 3
}

/// Preferences are optional. Also discard malformed/case-varied attempts so a
/// mistyped private credential cannot become a long-lived browser cookie.
pub fn private_preference(value: &str) -> bool {
    value.len() <= crate::MAX_PUBLIC_FIELD_BYTES
        && crate::posting_options::without_sage(value)
            .to_ascii_lowercase()
            .contains("capcode_!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_admission_matches_executed_source_cases() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vip-capcode.json")).unwrap();
        let cases = cases.as_array().unwrap();
        assert_eq!(cases.len(), 288);
        for case in cases {
            let prepared = crate::posting_options::without_sage(case["raw"].as_str().unwrap());
            let parsed = credentials(&prepared, case["bypass"].as_bool().unwrap());
            let lookup = if failure_limit_reached(case["failures"].as_u64().unwrap()) {
                None
            } else {
                parsed
            };
            assert_eq!(
                lookup.as_ref().map(|value| value.identifier()),
                case["effects"]["identifier"].as_str()
            );
            assert_eq!(
                u64::from(lookup.is_some()),
                case["effects"]["lookups"].as_u64().unwrap()
            );
        }
    }

    #[test]
    fn prepared_vip_options_preserve_source_split_and_php_truthiness() {
        for (raw, expected) in [
            ("capcode_!owner!key", Some(("owner", "key"))),
            ("capcode_!owner!key!tail", Some(("owner", "key!tail"))),
            ("capcode_!owner!!", Some(("owner", "!"))),
            ("capcode_!00!00", Some(("00", "00"))),
            ("capcode_! ! ", Some((" ", " "))),
            ("capcode_!名前!秘密", Some(("名前", "秘密"))),
            ("capcode_!owner", None),
            ("capcode_!!key", None),
            ("capcode_!owner!", None),
            ("capcode_!0!key", None),
            ("capcode_!owner!0", None),
            ("CAPCODE_!owner!key", None),
            (" capcode_!owner!key", None),
            ("capcode_admin", None),
            ("", None),
        ] {
            let actual = credentials(raw, true);
            assert_eq!(
                actual.as_ref().map(|v| (v.identifier(), v.password())),
                expected
            );
            assert!(credentials(raw, false).is_none());
        }
    }

    #[test]
    fn caller_removes_sage_before_reading_the_credential() {
        let prepared = crate::posting_options::without_sage("sagecapcode_!SAGEowner!mesSAGEkey");
        let parsed = credentials(&prepared, true).unwrap();
        assert_eq!(parsed.identifier(), "owner");
        assert_eq!(parsed.password(), "meskey");
    }

    #[test]
    fn public_byte_bound_and_failure_limit_remain_finite() {
        let prefix = "capcode_!owner!";
        let bounded = format!("{prefix}{}", "x".repeat(100 - prefix.len()));
        assert!(credentials(&bounded, true).is_some());
        assert!(credentials(&(bounded + "x"), true).is_none());
        for count in [0, 1, 2] {
            assert!(!failure_limit_reached(count));
        }
        for count in [3, 4, u64::MAX] {
            assert!(failure_limit_reached(count));
        }
    }

    #[test]
    fn private_preferences_include_mistyped_credential_attempts() {
        for value in [
            "capcode_!owner!secret",
            "CAPCODE_!owner!secret",
            " capcode_!owner!secret",
            "capsagecode_!owner!secret",
            "capcode_!owner",
        ] {
            assert!(private_preference(value));
        }
        for value in ["sage", "nonoko", "capcode_admin", "since4pass"] {
            assert!(!private_preference(value));
        }
    }
}
