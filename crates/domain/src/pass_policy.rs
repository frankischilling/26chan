//! Source Pass state decisions, after independent credential/session validation.
//! These values are policy results, never posting authority or proof of identity.

pub const LOGIN_NETWORK_HOLD_SECONDS: i64 = 900;
pub const POST_NETWORK_HOLD_SECONDS: i64 = 1800;
pub const LOGIN_FAILURE_LIMIT: u32 = 5;
pub const SESSION_SECONDS: i64 = 86400;
pub const REMEMBERED_SESSION_SECONDS: i64 = 31556900;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Active,
    Expired,
    Refunded,
    Disputed,
    RevokedSpam,
    RevokedIllegal,
    New,
    Delayed,
}
impl TryFrom<i16> for Status {
    type Error = ();
    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Active),
            1 => Ok(Self::Expired),
            2 => Ok(Self::Refunded),
            3 => Ok(Self::Disputed),
            4 => Ok(Self::RevokedSpam),
            5 => Ok(Self::RevokedIllegal),
            6 => Ok(Self::New),
            7 => Ok(Self::Delayed),
            _ => Err(()),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Login {
    Continue,
    Activate,
    ActivateYear,
    InUse { remaining: i64 },
    Disabled(Status),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Posting {
    Continue,
    Expired,
    Disabled,
    InUse,
}

/// The login reference compares IPv4 /16 networks, whereas posting compares /24.
/// A network provider supplies this comparison for the corresponding route.
/// An unset previous network does not impose a hold. No address is disclosed.
#[derive(Clone, Copy, Debug)]
pub struct NetworkUse {
    pub previous_known: bool,
    pub same_scope: bool,
    pub last_used: i64,
}
impl NetworkUse {
    fn remaining(self, now: i64, hold: i64) -> Option<i64> {
        if !self.previous_known || self.same_scope {
            return None;
        }
        let elapsed = now.saturating_sub(self.last_used);
        (elapsed < hold).then(|| hold.saturating_sub(elapsed))
    }
}

pub fn authenticated_login(status: Status, use_: NetworkUse, now: i64) -> Login {
    if let Some(remaining) = use_.remaining(now, LOGIN_NETWORK_HOLD_SECONDS) {
        return Login::InUse { remaining };
    }
    match status {
        Status::Active => Login::Continue,
        Status::New => Login::Activate,
        Status::Delayed => Login::ActivateYear,
        other => Login::Disabled(other),
    }
}

pub fn authenticated_posting(status: Status, expires: i64, use_: NetworkUse, now: i64) -> Posting {
    if expires <= now {
        return Posting::Expired;
    }
    if status != Status::Active {
        return Posting::Disabled;
    }
    if use_.remaining(now, POST_NETWORK_HOLD_SECONDS).is_some() {
        return Posting::InUse;
    }
    Posting::Continue
}

/// Threads keep their ordinary cooldown. Reply and duplicate-image intervals
/// use the source's ceiling division when a trusted bypass has been resolved.
pub fn reply_cooldown(seconds: u32) -> u32 {
    seconds.div_ceil(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn use_at(last_used: i64, same_scope: bool) -> NetworkUse {
        NetworkUse {
            previous_known: true,
            same_scope,
            last_used,
        }
    }
    #[test]
    fn login_transitions_and_disabled_states_are_distinct() {
        let same = use_at(1000, true);
        assert_eq!(
            authenticated_login(Status::Active, same, 1000),
            Login::Continue
        );
        assert_eq!(
            authenticated_login(Status::New, same, 1000),
            Login::Activate
        );
        assert_eq!(
            authenticated_login(Status::Delayed, same, 1000),
            Login::ActivateYear
        );
        for status in [
            Status::Expired,
            Status::Refunded,
            Status::Disputed,
            Status::RevokedSpam,
            Status::RevokedIllegal,
        ] {
            assert_eq!(
                authenticated_login(status, same, 1000),
                Login::Disabled(status)
            );
        }
        assert!(Status::try_from(-1).is_err());
        assert!(Status::try_from(8).is_err());
    }
    #[test]
    fn network_holds_use_distinct_exact_boundaries() {
        for elapsed in [0, 899, 900, 1799, 1800, 1801] {
            let network = use_at(1000, false);
            let login = authenticated_login(Status::Active, network, 1000 + elapsed);
            assert_eq!(matches!(login, Login::InUse { .. }), elapsed < 900);
            let post = authenticated_posting(Status::Active, 10000, network, 1000 + elapsed);
            assert_eq!(post == Posting::InUse, elapsed < 1800);
        }
        let unknown = NetworkUse {
            previous_known: false,
            ..use_at(1000, false)
        };
        assert_eq!(
            authenticated_login(Status::Active, unknown, 1000),
            Login::Continue
        );
        assert_eq!(
            authenticated_posting(Status::Active, 10000, unknown, 1000),
            Posting::Continue
        );
    }
    #[test]
    fn source_order_keeps_login_network_before_status_and_post_expiry_first() {
        let network = use_at(1000, false);
        assert_eq!(
            authenticated_login(Status::Expired, network, 1000),
            Login::InUse { remaining: 900 }
        );
        assert_eq!(
            authenticated_posting(Status::Refunded, 1000, network, 1000),
            Posting::Expired
        );
        assert_eq!(
            authenticated_posting(Status::Refunded, 1001, network, 1000),
            Posting::Disabled
        );
        assert_eq!(
            authenticated_posting(Status::Active, 1001, network, 1000),
            Posting::InUse
        );
    }
    #[test]
    fn cooldown_rounds_up_without_overflow() {
        for value in 0..1000 {
            let discounted = reply_cooldown(value);
            assert!(discounted * 2 >= value);
            assert!(discounted == 0 || (discounted - 1) * 2 < value);
        }
        assert_eq!(reply_cooldown(u32::MAX), 2147483648);
    }
}
