#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    Janitor = 1,
    Moderator = 10,
    Manager = 20,
    Admin = 50,
}

impl Level {
    pub fn parse(role: &str) -> Option<Self> {
        match role {
            "janitor" => Some(Self::Janitor),
            "mod" | "moderator" => Some(Self::Moderator),
            "manager" => Some(Self::Manager),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    pub fn role(self) -> &'static str {
        match self {
            Self::Janitor => "janitor",
            Self::Moderator => "moderator",
            Self::Manager => "manager",
            Self::Admin => "admin",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Janitor => "Janitor",
            Self::Moderator => "Mod",
            Self::Manager => "Manager",
            Self::Admin => "Admin",
        }
    }

    pub fn has_capability(self, capability: &str, permissions: &Permissions) -> bool {
        match capability {
            "delete" | "clear" => true,
            "ban_req" | "escalate" => matches!(self, Self::Janitor | Self::Manager | Self::Admin),
            "delete_ip" | "ban" | "ban_reporter" | "clear_reporter" | "delete_reporter"
            | "admin_php" | "cleanup" | "clear_alert" => self >= Self::Moderator,
            "is_manager" => self >= Self::Manager,
            "is_admin" | "nuke" | "manual_ban" => self == Self::Admin,
            "is_developer" => self == Self::Moderator && permissions.has_flag("developer"),
            _ => false,
        }
    }
}

/// The source grants a board when it is explicitly allowed or `all` is allowed.
/// An explicit deny wins, including the separate `noboard` permission.
#[derive(Clone, Debug, Default, sqlx::FromRow)]
pub struct Permissions {
    pub allow_boards: Vec<String>,
    pub deny_boards: Vec<String>,
    pub flags: Vec<String>,
}

impl Permissions {
    pub fn can_discuss(&self, role: &str) -> bool {
        Level::parse(role).is_some() && !self.deny_boards.iter().any(|board| board == "j")
    }

    pub fn all_boards() -> Self {
        Self {
            allow_boards: vec!["all".into()],
            ..Self::default()
        }
    }

    pub fn allows_all(&self) -> bool {
        self.allow_boards.iter().any(|board| board == "all")
    }

    pub fn allows(&self, board: &str) -> bool {
        let key = if board.is_empty() { "noboard" } else { board };
        (self.allows_all() || self.allow_boards.iter().any(|allowed| allowed == board))
            && !self.deny_boards.iter().any(|denied| denied == key)
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|value| value == flag)
    }

    pub fn action_allowed(&self, role: &str, board: &str, action: &str) -> bool {
        let Some(level) = Level::parse(role) else {
            return false;
        };
        if !self.allows(board) {
            return false;
        }
        match action {
            "remove-post" | "remove-file" | "remove-thread" | "resolve" | "dismiss" => true,
            "close" | "reopen" | "sticky" | "unsticky" | "permasage" | "unpermasage" => {
                level >= Level::Moderator
            }
            "permaage" | "unpermaage" => level == Level::Admin,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_levels_do_not_turn_the_developer_flag_into_a_rank() {
        for (role, value) in [("janitor", 1), ("mod", 10), ("manager", 20), ("admin", 50)] {
            assert_eq!(Level::parse(role).unwrap() as i32, value);
        }
        assert_eq!(Level::parse("developer"), None);
        assert_eq!(Level::parse("MOD"), None);
        let mut permissions = Permissions::all_boards();
        assert!(!Level::Moderator.has_capability("is_developer", &permissions));
        permissions.flags.push("developer".into());
        assert!(Level::Moderator.has_capability("is_developer", &permissions));
        assert!(!Level::Janitor.has_capability("is_developer", &permissions));
        assert!(!Level::Moderator.has_capability("ban_req", &permissions));
        assert!(Level::Manager.has_capability("ban_req", &permissions));
        assert!(!Level::Manager.has_capability("nuke", &permissions));
        assert!(Level::Admin.has_capability("nuke", &permissions));
    }

    #[test]
    fn board_scope_denials_and_janitor_actions_follow_source_access_rules() {
        let mut permissions = Permissions {
            allow_boards: vec!["a".into(), "g".into(), "janitor".into()],
            ..Permissions::default()
        };
        assert!(permissions.allows("a"));
        assert!(!permissions.allows("aa"));
        assert!(!permissions.allows("j"));
        assert!(!permissions.allows(""));
        assert!(permissions.action_allowed("janitor", "a", "remove-post"));
        assert!(permissions.action_allowed("janitor", "a", "resolve"));
        assert!(!permissions.action_allowed("janitor", "a", "close"));
        assert!(!permissions.action_allowed("moderator", "b", "close"));
        permissions.allow_boards.push("all".into());
        permissions
            .deny_boards
            .extend(["g".into(), "noboard".into()]);
        assert!(permissions.allows("b"));
        assert!(!permissions.allows("g"));
        assert!(!permissions.allows(""));
        assert!(!permissions.action_allowed("admin", "g", "remove-thread"));
        assert!(!permissions.action_allowed("unknown", "a", "remove-post"));
        assert!(!permissions.action_allowed("admin", "a", "invented-action"));
    }
}
