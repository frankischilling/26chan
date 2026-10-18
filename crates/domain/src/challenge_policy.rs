//! Source-visible challenge policy. All reputation inputs are server-owned.
//! Persistence, proof validation and atomic consumption belong to the store.
use crate::anonymous_session::State;

pub const CHALLENGE_SECONDS: u64 = 120;
pub const TICKET_SECONDS: u64 = 3600;
pub const UNSOLVED_SECONDS: u64 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketDecision {
    NotRequired,
    Issue { seconds: u16 },
    Wait { seconds: u16 },
    Ready,
}

/// Evaluate an already authenticated, board- and peer-bound ticket using the
/// store's clock. A browser timestamp is never an admissible `issued_at`.
/// Changing the required wait does not reset a live ticket's starting time.
pub fn ticket_decision(required: u16, issued_at: Option<u64>, now: u64) -> TicketDecision {
    if required == 0 {
        return TicketDecision::NotRequired;
    }
    let Some(age) = issued_at.and_then(|issued| now.checked_sub(issued)) else {
        return TicketDecision::Issue { seconds: required };
    };
    if age >= TICKET_SECONDS {
        return TicketDecision::Issue { seconds: required };
    }
    if age < u64::from(required) {
        return TicketDecision::Wait {
            seconds: required - age as u16,
        };
    }
    TicketDecision::Ready
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Thread,
    Reply,
    Report,
    SignIn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreditGrant {
    pub count: u8,
    pub seconds: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageStyle {
    pub slider: bool,
    pub characters: u8,
}

pub fn refresh_cooldown(unsolved: u32) -> u64 {
    match unsolved {
        0..=2 => 5,
        3..=10 => u64::from(unsolved) * 8,
        _ => 300,
    }
}

pub fn pre_cooldown(state: &State, purpose: Purpose, now: u64) -> u64 {
    if purpose == Purpose::SignIn {
        return 0;
    }
    let address_changed = state.created_at != now && state.address_at == now;
    let network_changed = state.created_at != now && state.network_at == now;
    let mut seconds = match purpose {
        Purpose::Thread
            if state.thread_count() < 1 || (address_changed && state.post_count() < 2) =>
        {
            300
        }
        Purpose::Reply if state.post_count() < 1 || (address_changed && state.post_count() < 2) => {
            60
        }
        Purpose::Report if state.report_count() < 1 && state.post_count() < 1 => 60,
        _ => 0,
    };
    if purpose != Purpose::Report && state.verified_level == 0 {
        if state.post_count() < 1 || !state.is_known(now, 15, 0) {
            seconds = 900;
        } else if state.password_age(now) < 86400 && network_changed {
            seconds = 180;
        }
    }
    seconds
}

/// Eligibility alone never grants or consumes a credit. A successful protected
/// write and the board's enabled policy are separate prerequisites.
pub fn credit_grant(state: &State, now: u64) -> Option<CreditGrant> {
    if now == 0 || state.address_age(now) < 60 {
        return None;
    }
    for (minutes, posts, count, seconds) in [
        (129600, 100, 3, 10800),
        (21600, 20, 2, 7200),
        (4320, 5, 1, 3600),
    ] {
        if state.is_known(now, minutes, 0) && state.post_count() >= posts {
            return Some(CreditGrant { count, seconds });
        }
    }
    None
}

pub fn credit_eligible(
    state: &State,
    now: u64,
    purpose: Purpose,
    board: &str,
    board_enabled: bool,
    suspicious: bool,
) -> bool {
    board_enabled
        && !suspicious
        && matches!(purpose, Purpose::Reply | Purpose::Report)
        && !matches!(board, "bant" | "pol" | "trash")
        && state.is_known(now, 4320, 0)
        && state.post_count() >= 5
}

pub fn image_style(state: &State, now: u64, purpose: Purpose, suspicious: bool) -> ImageStyle {
    if purpose != Purpose::Thread
        && !suspicious
        && state.is_known(now, 4320, 0)
        && state.post_count() >= 5
    {
        if state.verified_level > 0 {
            return ImageStyle {
                slider: purpose == Purpose::SignIn,
                characters: 4,
            };
        }
        let idle = if state.activity_at > 0 {
            now.saturating_sub(state.activity_at)
        } else {
            state.created_at
        };
        let changed = state.created_at != now && state.address_at == now;
        if idle <= 86400 && !changed {
            return ImageStyle {
                slider: purpose == Purpose::SignIn,
                characters: 5,
            };
        }
    }
    ImageStyle {
        slider: true,
        characters: 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_keep_elapsed_wait_until_the_exact_expiry_boundary() {
        let start = 10_000;
        assert_eq!(
            ticket_decision(900, None, start),
            TicketDecision::Issue { seconds: 900 }
        );
        assert_eq!(
            ticket_decision(900, Some(start), start),
            TicketDecision::Wait { seconds: 900 }
        );
        assert_eq!(
            ticket_decision(900, Some(start), start + 899),
            TicketDecision::Wait { seconds: 1 }
        );
        assert_eq!(
            ticket_decision(900, Some(start), start + 900),
            TicketDecision::Ready
        );
        assert_eq!(
            ticket_decision(900, Some(start), start + TICKET_SECONDS - 1),
            TicketDecision::Ready
        );
        assert_eq!(
            ticket_decision(900, Some(start), start + TICKET_SECONDS),
            TicketDecision::Issue { seconds: 900 }
        );
        assert_eq!(
            ticket_decision(60, Some(start), start + 60),
            TicketDecision::Ready
        );
        assert_eq!(
            ticket_decision(300, Some(start), start + 60),
            TicketDecision::Wait { seconds: 240 }
        );
    }

    #[test]
    fn absent_future_and_expired_tickets_do_not_skip_a_wait() {
        for issued in [None, Some(101), Some(u64::MAX)] {
            assert_eq!(
                ticket_decision(60, issued, 100),
                TicketDecision::Issue { seconds: 60 }
            );
            assert_eq!(ticket_decision(0, issued, 100), TicketDecision::NotRequired);
        }
        assert_eq!(
            ticket_decision(60, Some(1), u64::MAX),
            TicketDecision::Issue { seconds: 60 }
        );
        assert_eq!(
            ticket_decision(0, Some(1), u64::MAX),
            TicketDecision::NotRequired
        );
    }

    #[test]
    fn refresh_limits_have_exact_source_boundaries() {
        for (count, expected) in [
            (0, 5),
            (2, 5),
            (3, 24),
            (10, 80),
            (11, 300),
            (u32::MAX, 300),
        ] {
            assert_eq!(refresh_cooldown(count), expected);
        }
    }
    #[test]
    fn fresh_and_verified_users_preserve_action_specific_waits() {
        let now = 10_000_000;
        let mut state = State::new(now);
        assert_eq!(pre_cooldown(&state, Purpose::Thread, now), 900);
        assert_eq!(pre_cooldown(&state, Purpose::Reply, now), 900);
        assert_eq!(pre_cooldown(&state, Purpose::Report, now), 60);
        assert_eq!(pre_cooldown(&state, Purpose::SignIn, now), 0);
        state.verified_level = 1;
        assert_eq!(pre_cooldown(&state, Purpose::Thread, now), 300);
        assert_eq!(pre_cooldown(&state, Purpose::Reply, now), 60);
        assert_eq!(credit_grant(&state, now), None);
    }
    #[test]
    fn credit_tiers_and_board_restrictions_are_independent() {
        let now = 10_000_000;
        for (minutes, posts, count, seconds) in [
            (4320, 5, 1, 3600),
            (21600, 20, 2, 7200),
            (129600, 100, 3, 10800),
        ] {
            let mut state = State::new(now - minutes * 60);
            state.posts = posts;
            assert_eq!(
                credit_grant(&state, now),
                Some(CreditGrant { count, seconds })
            );
            assert!(credit_eligible(
                &state,
                now,
                Purpose::Reply,
                "g",
                true,
                false
            ));
            for board in ["bant", "pol", "trash"] {
                assert!(!credit_eligible(
                    &state,
                    now,
                    Purpose::Reply,
                    board,
                    true,
                    false
                ));
            }
            assert!(!credit_eligible(
                &state,
                now,
                Purpose::Thread,
                "g",
                true,
                false
            ));
            assert!(!credit_eligible(
                &state,
                now,
                Purpose::Reply,
                "g",
                false,
                false
            ));
            assert!(!credit_eligible(
                &state,
                now,
                Purpose::Reply,
                "g",
                true,
                true
            ));
            state.address_at = now - 59;
            assert_eq!(credit_grant(&state, now), None);
        }
    }
    #[test]
    fn threads_and_untrusted_risk_never_select_static_images() {
        let now = 10_000_000;
        let mut state = State::new(now - 4320 * 60);
        state.posts = 5;
        state.activity_at = now;
        assert_eq!(
            image_style(&state, now, Purpose::Reply, false),
            ImageStyle {
                slider: false,
                characters: 5
            }
        );
        state.verified_level = 1;
        assert_eq!(
            image_style(&state, now, Purpose::SignIn, false),
            ImageStyle {
                slider: true,
                characters: 4
            }
        );
        assert_eq!(
            image_style(&state, now, Purpose::Reply, false),
            ImageStyle {
                slider: false,
                characters: 4
            }
        );
        assert_eq!(
            image_style(&state, now, Purpose::Thread, false),
            ImageStyle {
                slider: true,
                characters: 6
            }
        );
        assert_eq!(
            image_style(&state, now, Purpose::Reply, true),
            ImageStyle {
                slider: true,
                characters: 6
            }
        );
    }
}

#[cfg(test)]
#[path = "challenge_policy/credit_score_tests.rs"]
mod credit_score_tests;
