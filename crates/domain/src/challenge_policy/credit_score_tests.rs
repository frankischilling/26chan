use super::*;
use crate::anonymous_session::Changes;

// The source checks known-or-verified for six hours before deciding whether to
// calculate a score. Every credit tier requires plain knownness for >=3 days.
// Both use the same pre-post state and clock; no numeric score is invented here.
#[test]
fn every_credit_tier_implies_the_six_hour_score_bypass() {
    let now = 10_000_000;
    let ages = [
        0, 1799, 1800, 21599, 21600, 259199, 259200, 1295999, 1296000, 7775999, 7776000,
    ];
    let mut tiers = [0_usize; 3];
    for password_age in ages {
        for network_age in ages {
            for posts in [0, 2, 4, 5, 8, 9, 19, 20, 99, 100] {
                for reports in [0, 9, 10, 19, 20] {
                    for pending in [0, 1, 8, 9] {
                        for change_score in [0, 9, 10, 32] {
                            for verified_level in [0, 1] {
                                let state = State {
                                    created_at: now - password_age,
                                    network_at: now - network_age,
                                    address_at: now - 60,
                                    posts,
                                    reports,
                                    pending,
                                    change_score,
                                    verified_level,
                                    ..State::new(now)
                                };
                                if let Some(grant) = credit_grant(&state, now) {
                                    assert!(state.is_known(now, 360, 0));
                                    assert!(state.is_known_or_verified(now, 360, 0));
                                    tiers[usize::from(grant.count) - 1] += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        tiers.into_iter().all(|count| count > 0),
        "All three tiers must be exercised"
    );
}

#[test]
fn resumption_and_unknown_user_inputs_do_not_reverse_the_implication() {
    let now = 10_000_000;
    for idle in [604799, 604800] {
        for network in [false, true] {
            for address in [false, true] {
                for environment in [false, true] {
                    let mut state = State {
                        created_at: now - 7_776_000,
                        network_at: now - 7_776_000,
                        address_at: now - 60,
                        activity_at: now - idle,
                        posts: 100,
                        pending: 1,
                        ..State::new(now)
                    };
                    let reset = state.resume(
                        now,
                        Changes {
                            network,
                            address,
                            environment,
                        },
                    );
                    let grant = credit_grant(&state, now);
                    if reset {
                        assert!(grant.is_none());
                    }
                    if grant.is_some() {
                        assert!(state.is_known_or_verified(now, 360, 0));
                    }
                }
            }
        }
    }
    let mut verified = State::new(now);
    verified.verified_level = 1;
    assert!(verified.is_known_or_verified(now, 360, 0));
    assert!(
        credit_grant(&verified, now).is_none(),
        "The implication is not an authority to grant credits"
    );
    let mut young = State::new(now);
    young.created_at = now - 21600;
    young.network_at = now - 21600;
    young.address_at = now - 60;
    young.posts = 100;
    assert!(young.is_known_or_verified(now, 360, 0));
    assert!(
        credit_grant(&young, now).is_none(),
        "Six-hour knownness alone cannot satisfy a credit tier"
    );
}
