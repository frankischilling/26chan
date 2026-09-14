/// Source bump indicators exclude sticky/permaage, but not permasage threads.
pub fn limited(sticky: bool, permaage: bool, replies: u64, limit: u32) -> bool {
    !sticky && !permaage && replies >= u64::from(limit)
}

/// The source checks the count after inserting the incoming reply.
/// Sticky/permasage take precedence over permaage, which overrides age,
/// sage and the surviving reply count. Self-bump policies are separate.
pub fn should_bump(
    sticky: bool,
    permasage: bool,
    permaage: bool,
    sage: bool,
    replies_after: u64,
    limit: u32,
    age_limited: bool,
) -> bool {
    !sticky
        && !permasage
        && (permaage || (!sage && !age_limited && replies_after < u64::from(limit)))
}

/// PERMASAGE_HOURS compares whole request-start and OP creation seconds.
/// Zero disables the policy; equality suppresses the ordinary bump.
pub fn age_limited(request_seconds: i64, created_seconds: i64, hours: u32) -> bool {
    hours != 0
        && i128::from(request_seconds) - i128::from(hours) * 3600 >= i128::from(created_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surviving_post_insert_counts_obey_the_source_boundary() {
        for (count, limit, bump) in [
            (0, 0, false),
            (1, 1, false),
            (1, 2, true),
            (2, 2, false),
            (3, 2, false),
        ] {
            assert_eq!(
                should_bump(false, false, false, false, count, limit, false),
                bump
            );
            assert_eq!(limited(false, false, count, limit), !bump);
            assert!(!should_bump(false, false, false, true, count, limit, false));
            assert!(!should_bump(true, false, false, false, count, limit, false));
            assert!(!limited(true, false, count, limit));
        }
        assert!(!should_bump(false, false, false, false, 3, 3, false));
        assert!(
            should_bump(false, false, false, false, 2, 3, false),
            "deleted replies no longer count"
        );
        assert!(limited(false, false, u64::MAX, u32::MAX));
    }

    #[test]
    fn flag_precedence_is_independent_of_sage_and_count() {
        for sticky in [false, true] {
            for permasage in [false, true] {
                for permaage in [false, true] {
                    for sage in [false, true] {
                        for count in [0, 1, 2, u64::MAX] {
                            for limit in [0, 1, 2, u32::MAX] {
                                let expected = if sticky || permasage {
                                    false
                                } else if permaage {
                                    true
                                } else {
                                    !sage && count < u64::from(limit)
                                };
                                assert_eq!(
                                    should_bump(
                                        sticky, permasage, permaage, sage, count, limit, false
                                    ),
                                    expected
                                );
                                assert_eq!(
                                    should_bump(
                                        sticky, permasage, permaage, sage, count, limit, true
                                    ),
                                    !sticky && !permasage && permaage
                                );
                                assert_eq!(
                                    limited(sticky, permaage, count, limit),
                                    !sticky && !permaage && count >= u64::from(limit)
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn age_boundary_is_whole_seconds_and_overflow_safe() {
        for hours in [1, 48, 120, 168, 336, u32::MAX] {
            let cutoff = 1000 + i64::from(hours) * 3600;
            assert!(!age_limited(cutoff - 1, 1000, hours));
            assert!(age_limited(cutoff, 1000, hours));
            assert!(age_limited(cutoff + 1, 1000, hours));
            assert!(!age_limited(999, 1000, hours));
        }
        assert!(!age_limited(i64::MAX, i64::MIN, 0));
        assert!(age_limited(i64::MAX, i64::MIN, u32::MAX));
        assert!(!age_limited(i64::MIN, i64::MAX, u32::MAX));
        assert!(!age_limited(i64::MIN, i64::MIN, 1));
    }
}
