#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    Janitor = 1,
    Moderator = 10,
    Manager = 20,
    Admin = 50,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapcodeError {
    MissingPermission,
}

/// Prepared source Options policy, separate from current session/board authority.
/// Construct this from the authenticated account, never from a serialized proof.
#[derive(Debug, Eq, PartialEq)]
pub struct StaffPostingOptions {
    pub capcode: Option<board_domain::capcode::Capcode>,
    pub name_allowed: bool,
    pub sage: bool,
    pub return_to_board: bool,
    pub options_field: String,
    pub authorized_limits: bool,
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

    /// Source badge selection is separate from board posting authorization.
    pub fn public_capcode(
        self,
        options: &str,
        permissions: &Permissions,
    ) -> Result<Option<board_domain::capcode::Capcode>, CapcodeError> {
        use board_domain::capcode::Capcode;
        if self < Self::Moderator {
            return Ok(None);
        }
        let selected = match options {
            "capcode_founder" if self == Self::Admin => Some(Capcode::Founder),
            "capcode_admin" if self == Self::Admin => Some(Capcode::Administrator),
            "capcode_admin_hl" if self == Self::Admin => Some(Capcode::HighlightedAdministrator),
            "capcode_dev" if permissions.has_global_flag("developer") => Some(Capcode::Developer),
            "capcode_manager" if self >= Self::Manager => Some(Capcode::Manager),
            _ => None,
        };
        if selected.is_some() {
            return Ok(selected);
        }
        if self < Self::Manager && !permissions.has_global_flag("capcode") && !options.is_empty() {
            return Err(CapcodeError::MissingPermission);
        }
        Ok((options == "capcode_mod").then_some(Capcode::Moderator))
    }

    pub fn allows_capcode_name(self, permissions: &Permissions) -> bool {
        self == Self::Admin || permissions.has_global_flag("capcodename")
    }

    pub fn posting_options(
        self,
        raw: &str,
        permissions: &Permissions,
    ) -> Result<StaffPostingOptions, board_domain::ValidationError> {
        let parsed = board_domain::posting_options::parse(raw)?;
        let options_field = board_domain::posting_options::without_sage(raw);
        let attempts_badge = options_field.starts_with("capcode_");
        let capcode = if attempts_badge {
            self.public_capcode(&options_field, permissions)
                .map_err(|_| board_domain::ValidationError("You cannot use that staff badge."))?
        } else {
            None
        };
        Ok(StaffPostingOptions {
            capcode,
            name_allowed: !attempts_badge || self.allows_capcode_name(permissions),
            sage: parsed.sage,
            return_to_board: parsed.return_to_board,
            options_field,
            authorized_limits: self >= Self::Moderator,
        })
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

    /// The selected source helpers call has_flag without a board argument.
    pub fn has_global_flag(&self, flag: &str) -> bool {
        self.allows("") && self.has_flag(flag)
    }

    pub fn can_set_permaage(&self, role: &str) -> bool {
        Level::parse(role).is_some_and(|level| {
            level >= Level::Moderator
                && (level >= Level::Manager || self.has_global_flag("developer"))
        })
    }

    pub fn action_allowed(&self, role: &str, board: &str, action: &str) -> bool {
        let Some(level) = Level::parse(role) else {
            return false;
        };
        if !self.allows(board) {
            return false;
        }
        match action {
            "remove-post" | "remove-file" | "remove-thread" | "resolve" | "dismiss" | "spoiler"
            | "unspoiler" => true,
            "close" | "reopen" | "sticky" | "unsticky" | "permasage" | "unpermasage" | "undead"
            | "unundead" | "thread-options" => level >= Level::Moderator,
            "permaage" | "unpermaage" => self.can_set_permaage(role),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_option_permissions_match_all_original_preparation_cases() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            role: String,
            developer: bool,
            allow_all: bool,
            deny_noboard: bool,
            thread_options_allowed: bool,
            permaage_allowed: bool,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/staff-thread-options.json"))
                .unwrap();
        assert_eq!(fixture.cases.len(), 512);
        for case in fixture.cases {
            let permissions = Permissions {
                allow_boards: vec![if case.allow_all { "all" } else { "g" }.into()],
                deny_boards: if case.deny_noboard {
                    vec!["noboard".into()]
                } else {
                    vec![]
                },
                flags: if case.developer {
                    vec!["developer".into()]
                } else {
                    vec![]
                },
            };
            assert_eq!(
                permissions.can_set_permaage(&case.role),
                case.thread_options_allowed && case.permaage_allowed,
                "{} {} {} {}",
                case.role,
                case.developer,
                case.allow_all,
                case.deny_noboard
            );
            for action in ["permaage", "unpermaage"] {
                assert_eq!(
                    permissions.action_allowed(&case.role, "g", action),
                    case.thread_options_allowed && case.permaage_allowed
                );
            }
            for action in ["undead", "unundead"] {
                assert_eq!(
                    permissions.action_allowed(&case.role, "g", action),
                    case.thread_options_allowed
                );
            }
        }
    }

    #[test]
    fn posting_options_match_the_full_source_preparation_order() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            role: String,
            flags: Vec<String>,
            allow_boards: Vec<String>,
            deny_boards: Vec<String>,
            input: String,
            options_field: String,
            sage: bool,
            return_to_board: bool,
            outcome: String,
            name: String,
            robot_applies: bool,
            authorized_limits: bool,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/staff-posting-options.json"))
                .unwrap();
        assert_eq!(fixture.cases.len(), 2048);
        for case in fixture.cases {
            let level = Level::parse(&case.role).unwrap();
            let permissions = Permissions {
                flags: case.flags,
                allow_boards: case.allow_boards,
                deny_boards: case.deny_boards,
            };
            let policy = level.posting_options(&case.input, &permissions);
            if case.outcome == "cant_capcode" {
                assert_eq!(policy.unwrap_err().0, "You cannot use that staff badge.");
                continue;
            }
            let policy = policy.unwrap();
            assert_eq!(
                policy.capcode.map_or("none", |badge| badge.as_str()),
                case.outcome
            );
            assert_eq!(policy.options_field, case.options_field);
            assert_eq!(policy.sage, case.sage);
            assert_eq!(policy.return_to_board, case.return_to_board);
            assert_eq!(policy.authorized_limits, case.authorized_limits);
            assert_eq!(
                if policy.name_allowed {
                    "Owned finished name"
                } else {
                    "Anonymous"
                },
                case.name
            );
            assert_eq!(
                board_domain::robot9000::applies_to_post(
                    true,
                    policy.capcode,
                    &policy.options_field,
                    true
                ),
                case.robot_applies,
                "{} {:?}",
                case.role,
                case.input
            );
        }
    }

    #[test]
    fn public_capcode_and_name_permissions_match_all_pinned_source_cases() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(serde::Deserialize)]
        struct Case {
            role: String,
            flags: Vec<String>,
            allow_boards: Vec<String>,
            deny_boards: Vec<String>,
            choice: String,
            outcome: String,
            name: String,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/staff-capcodes.json")).unwrap();
        assert_eq!(fixture.cases.len(), 1024);
        for case in fixture.cases {
            let level = Level::parse(&case.role).unwrap();
            let permissions = Permissions {
                flags: case.flags,
                allow_boards: case.allow_boards,
                deny_boards: case.deny_boards,
            };
            let outcome = match level.public_capcode(&case.choice, &permissions) {
                Ok(Some(capcode)) => capcode.as_str(),
                Ok(None) => "none",
                Err(CapcodeError::MissingPermission) => "cant_capcode",
            };
            assert_eq!(outcome, case.outcome, "{} {}", case.role, case.choice);
            let name = if case.choice.starts_with("capcode_")
                && !level.allows_capcode_name(&permissions)
            {
                "Anonymous"
            } else {
                "Owned finished name"
            };
            assert_eq!(name, case.name, "{} {}", case.role, case.choice);
        }
    }

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
