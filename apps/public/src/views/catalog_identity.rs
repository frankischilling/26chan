//! Catalog hover identity follows catalog.php and catalog.js, not post headers.

pub struct Badge<'a> {
    pub code: &'a str,
    pub label: String,
}

pub fn badge(code: Option<&str>, last_reply: bool) -> Option<Badge<'_>> {
    let code = code.filter(|code| !code.is_empty() && *code != "none")?;
    let label = if last_reply {
        let mut chars = code.chars();
        chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
    } else {
        // The supplied catalog.js capcodeMap omits admin_highlight: its OP
        // tooltip literally says "undefined", while replies say "Admin_highlight".
        // Do not normalize either through the ordinary post-header label.
        match code {
            "admin" => "Administrator",
            "mod" => "Moderator",
            "developer" => "Developer",
            "manager" => "Manager",
            "founder" => "Founder",
            "verified" => "Verified",
            _ => "undefined",
        }
        .to_owned()
    };
    Some(Badge { code, label })
}

pub fn country_class(
    capcode: Option<&str>,
    country: Option<&str>,
    country_flags: bool,
    board_flags_enabled: bool,
    board_flag: Option<&str>,
) -> Option<String> {
    // catalog.php exports countries only for unbadged OPs, and suppresses them
    // for any nonempty board flag when board flags are enabled (no flag lookup).
    if badge(capcode, false).is_some()
        || !country_flags
        || (board_flags_enabled && board_flag.is_some_and(|flag| !flag.is_empty()))
    {
        return None;
    }
    country
        .filter(|code| board_domain::country::country_code(code))
        .map(|code| format!("flag flag-{}", code.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_map_and_reply_capitalization_are_distinct() {
        for (code, op, reply) in [
            ("mod", "Moderator", "Mod"),
            ("admin", "Administrator", "Admin"),
            ("admin_highlight", "undefined", "Admin_highlight"),
            ("manager", "Manager", "Manager"),
            ("developer", "Developer", "Developer"),
            ("founder", "Founder", "Founder"),
            ("verified", "Verified", "Verified"),
        ] {
            let badge = super::badge(Some(code), false).unwrap();
            assert_eq!(badge.code, code);
            assert_eq!(badge.label, op);
            assert_eq!(super::badge(Some(code), true).unwrap().label, reply);
        }
        for code in [None, Some(""), Some("none")] {
            assert!(badge(code, false).is_none());
            assert!(badge(code, true).is_none());
        }
    }

    #[test]
    fn country_requires_unbadged_op_and_no_enabled_board_flag() {
        assert_eq!(
            country_class(None, Some("US"), true, false, None).as_deref(),
            Some("flag flag-us")
        );
        for code in [
            "mod",
            "admin",
            "admin_highlight",
            "manager",
            "developer",
            "founder",
        ] {
            assert!(country_class(Some(code), Some("US"), true, false, None).is_none());
        }
        assert!(country_class(None, Some("US"), false, false, None).is_none());
        assert!(country_class(None, Some("US"), true, true, Some("unknown")).is_none());
        assert!(country_class(None, Some("US"), true, false, Some("unknown")).is_some());
        assert!(country_class(None, Some("US"), true, true, Some("")).is_some());
        assert!(country_class(None, Some("US"), true, true, None).is_some());
        assert!(country_class(None, None, true, false, None).is_none());
        assert!(country_class(None, Some("invalid"), true, false, None).is_none());
    }
}
