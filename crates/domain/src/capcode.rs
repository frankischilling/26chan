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

#[cfg(test)]
mod tests {
    use super::*;

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
