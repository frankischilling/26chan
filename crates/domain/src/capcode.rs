//! Saved public staff labels; a label is never proof of request authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capcode {
    Moderator,
    Administrator,
    HighlightedAdministrator,
    Manager,
    Developer,
    Founder,
}

impl Capcode {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "mod" => Self::Moderator,
            "admin" => Self::Administrator,
            "admin_highlight" => Self::HighlightedAdministrator,
            "manager" => Self::Manager,
            "developer" => Self::Developer,
            "founder" => Self::Founder,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "mod",
            Self::Administrator => "admin",
            Self::HighlightedAdministrator => "admin_highlight",
            Self::Manager => "manager",
            Self::Developer => "developer",
            Self::Founder => "founder",
        }
    }

    pub fn source_option(self) -> &'static str {
        match self {
            Self::Moderator => "capcode_mod",
            Self::Administrator => "capcode_admin",
            Self::HighlightedAdministrator => "capcode_admin_hl",
            Self::Manager => "capcode_manager",
            Self::Developer => "capcode_dev",
            Self::Founder => "capcode_founder",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Moderator => "Mod",
            Self::Administrator | Self::HighlightedAdministrator => "Admin",
            Self::Manager => "Manager",
            Self::Developer => "Developer",
            Self::Founder => "Founder",
        }
    }

    pub fn name_class(self) -> &'static str {
        match self {
            Self::Moderator => "capcodeMod",
            Self::Administrator | Self::HighlightedAdministrator | Self::Founder => "capcodeAdmin",
            Self::Manager => "capcodeManager",
            Self::Developer => "capcodeDeveloper",
        }
    }

    pub fn highlight_id(self) -> &'static str {
        match self {
            Self::Moderator => "id_mod",
            Self::Administrator | Self::HighlightedAdministrator | Self::Founder => "id_admin",
            Self::Manager => "id_manager",
            Self::Developer => "id_developer",
        }
    }

    pub fn highlight_title(self) -> &'static str {
        match self {
            Self::Moderator => "Highlight posts by Moderators",
            Self::Administrator | Self::HighlightedAdministrator => {
                "Highlight posts by Administrators"
            }
            Self::Manager => "Highlight posts by Managers",
            Self::Developer => "Highlight posts by Developers",
            Self::Founder => "Highlight posts by the Founder",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Moderator => "modicon",
            Self::Administrator | Self::HighlightedAdministrator => "adminicon",
            Self::Manager => "managericon",
            Self::Developer => "developericon",
            Self::Founder => "foundericon",
        }
    }

    pub fn icon_title(self) -> &'static str {
        match self {
            Self::Moderator => "This user is a board Moderator.",
            Self::Administrator | Self::HighlightedAdministrator => {
                "This user is a board Administrator."
            }
            Self::Manager => "This user is a board Manager.",
            Self::Developer => "This user is a board Developer.",
            Self::Founder => "This user is the board's Founder.",
        }
    }

    pub fn highlighted(self) -> bool {
        self == Self::HighlightedAdministrator
    }
}

/// The catalog's identity suppression is narrower than posting's rank-based
/// administrator exception: only these two saved badge values bypass it.
pub fn catalog_identity_visible(
    capcode: Option<&str>,
    forced_anonymous: bool,
    meta_board: bool,
) -> bool {
    !(forced_anonymous || meta_board) || matches!(capcode, Some("admin" | "admin_highlight"))
}

#[derive(Debug, PartialEq, Eq)]
pub struct JsonIdentity<'a> {
    pub name: Option<&'a str>,
    pub trip: Option<&'a str>,
}

/// json.php checks the literal `admin_hl`, unlike catalog.php. Keep its
/// conditional field projection separate from the catalog and saved identity.
pub fn json_identity<'a>(
    name: &'a str,
    trip: Option<&'a str>,
    capcode: Option<&str>,
    forced_anonymous: bool,
    meta_board: bool,
) -> JsonIdentity<'a> {
    if (forced_anonymous || meta_board) && !matches!(capcode, Some("admin" | "admin_hl")) {
        JsonIdentity {
            name: Some("Anonymous"),
            trip: None,
        }
    } else {
        JsonIdentity {
            name: (!name.is_empty() || trip.is_none()).then_some(name),
            trip,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Badge reply groups exceed the thread limit.")]
pub struct CapcodeReplyLimit;

/// Group public saved labels only; these labels confer no posting authority.
pub fn capcode_reply_groups<'a>(
    replies: impl IntoIterator<Item = (i64, &'a str)>,
) -> Result<std::collections::BTreeMap<&'a str, Vec<i64>>, CapcodeReplyLimit> {
    let mut groups = std::collections::BTreeMap::new();
    for (index, (id, capcode)) in replies.into_iter().enumerate() {
        if index >= 1000 {
            return Err(CapcodeReplyLimit);
        }
        let group = match capcode {
            "none" => continue,
            "admin_highlight" => "admin",
            group => group,
        };
        groups.entry(group).or_insert_with(Vec::new).push(id);
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_identity_and_groups_match_the_pinned_source_blocks() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/staff-json.json")).unwrap();
        let cases = fixture["identity_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 108);
        for case in cases {
            let capcode = case["capcode"].as_str().unwrap();
            let identity = json_identity(
                case["name"].as_str().unwrap(),
                case["trip"].as_str(),
                (capcode != "none").then_some(capcode),
                case["forced_anonymous"].as_bool().unwrap(),
                case["meta_board"].as_bool().unwrap(),
            );
            let expected = &case["expected"];
            assert_eq!(
                identity.name,
                expected.get("name").and_then(|v| v.as_str()),
                "{case}"
            );
            assert_eq!(
                identity.trip,
                expected.get("trip").and_then(|v| v.as_str()),
                "{case}"
            );
        }
        let cases = fixture["reply_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 8);
        for case in cases {
            let groups =
                capcode_reply_groups(case["rows"].as_array().unwrap().iter().filter_map(|row| {
                    row["capcode"]
                        .as_str()
                        .map(|capcode| (row["id"].as_i64().unwrap(), capcode))
                }))
                .unwrap();
            let actual = if groups.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::to_value(groups).unwrap()
            };
            assert_eq!(actual, case["expected"], "{case}");
        }
    }

    #[test]
    fn badge_reply_groups_keep_the_existing_thousand_reply_bound() {
        let groups = capcode_reply_groups((1..=1000).map(|id| (id, "admin_highlight"))).unwrap();
        assert_eq!(groups["admin"], (1..=1000).collect::<Vec<_>>());
        assert!(capcode_reply_groups((1..=1001).map(|id| (id, "mod"))).is_err());
    }

    #[test]
    fn catalog_identity_matches_both_pinned_source_predicates() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/staff-catalog-identity.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 64);
        for case in cases {
            let badge = case["capcode"].as_str().unwrap();
            let capcode = (badge != "none").then_some(badge);
            assert_eq!(
                catalog_identity_visible(
                    capcode,
                    case["forced_anonymous"].as_bool().unwrap(),
                    case["meta_board"].as_bool().unwrap()
                ),
                case["identity_visible"].as_bool().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn only_pinned_labels_have_fixed_rendering_values() {
        for value in [
            "mod",
            "admin",
            "admin_highlight",
            "manager",
            "developer",
            "founder",
        ] {
            let capcode = Capcode::parse(value).unwrap();
            assert_eq!(capcode.as_str(), value);
            assert!(capcode.name_class().starts_with("capcode"));
            assert!(!capcode.icon().contains('/'));
            assert_eq!(capcode.highlighted(), value == "admin_highlight");
        }
        assert_eq!(Capcode::Founder.highlight_id(), "id_admin");
        assert_eq!(Capcode::HighlightedAdministrator.label(), "Admin");
        for value in [
            "",
            "Admin",
            "verified",
            "admin admin_highlight",
            "<script>",
            "mod\0",
        ] {
            assert_eq!(Capcode::parse(value), None);
        }
    }
}
