//! Ordered source posting restriction policy, without identity or staff authority.
//! Matches and entitlement facts must be supplied by a trusted boundary. This
//! function does not authenticate a Pass, verify a user, or authorize a ban.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchKey {
    Address,
    DeletionCredential,
    Pass,
}

#[derive(Clone, Copy, Debug)]
pub struct MatchedRestriction<'a> {
    pub id: i64,
    pub key: MatchKey,
    pub global: bool,
    pub board: &'a str,
    pub starts_at: i64,
    /// None is the legacy zero-date indefinite restriction.
    pub ends_at: Option<i64>,
    pub template_id: i64,
    pub automatic: bool,
    /// Complete persisted reason, including its private component. Never a
    /// public rendering value: the source searches both parts for "Proxy".
    pub reason: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    Allowed,
    Warned,
    Banned,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Decision {
    pub disposition: Disposition,
    /// Only expired rows reached before the source's first blocking match.
    /// The caller still needs authority and concurrency control to retire them.
    pub expired: Vec<i64>,
}

#[derive(Clone, Copy, Debug)]
pub struct QualifiedContext<'a> {
    pub board: &'a str,
    pub request_at: i64,
    pub pass_present: bool,
    pub captcha_bypass: bool,
    pub verified_user: bool,
}

/// Matches must retain the database result order within each field and the
/// caller's field order, with address matches last (as in the source). Active
/// exact-identity matching belongs to the private persistence boundary.
pub fn decide(context: QualifiedContext<'_>, matches: &[MatchedRestriction<'_>]) -> Decision {
    let mut decision = Decision {
        disposition: Disposition::Allowed,
        expired: Vec::new(),
    };
    for restriction in matches {
        if (restriction.key == MatchKey::Address && context.pass_present && context.captcha_bypass)
            || (restriction.key == MatchKey::Pass && !context.pass_present)
        {
            continue;
        }
        if let Some(end) = restriction.ends_at.filter(|&end| end != 0) {
            // Compare rather than subtract: persisted timestamps must not
            // overflow signed arithmetic, and the source's condition is < 1.
            if end <= restriction.starts_at {
                decision.disposition = Disposition::Warned;
                break;
            }
            if end <= context.request_at {
                decision.expired.push(restriction.id);
                continue;
            }
        }
        if restriction.key == MatchKey::Pass && restriction.template_id == 124 {
            continue;
        }
        if context.verified_user
            && restriction.key == MatchKey::Address
            && restriction.automatic
            && restriction.reason.contains("Proxy")
        {
            continue;
        }
        if restriction.global || restriction.board == context.board {
            decision.disposition = Disposition::Banned;
            break;
        }
    }
    decision
}
