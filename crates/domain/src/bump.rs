/// Source bump indicators exclude sticky/permaage, but not permasage threads.
pub fn limited(sticky: bool, permaage: bool, replies: u64, limit: u32) -> bool {
    !sticky && !permaage && replies >= u64::from(limit)
}

/// The source checks the count after inserting the incoming reply.
/// Sticky/permasage take precedence over permaage. Board age/self-bump
/// policies are separate rules, not represented by this count/flag decision.
pub fn should_bump(
    sticky: bool,
    permasage: bool,
    permaage: bool,
    sage: bool,
    replies_after: u64,
    limit: u32,
) -> bool {
    !sticky && !permasage && (permaage || (!sage && replies_after < u64::from(limit)))
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
            assert_eq!(should_bump(false, false, false, false, count, limit), bump);
            assert_eq!(limited(false, false, count, limit), !bump);
            assert!(!should_bump(false, false, false, true, count, limit));
            assert!(!should_bump(true, false, false, false, count, limit));
            assert!(!limited(true, false, count, limit));
        }
        assert!(!should_bump(false, false, false, false, 3, 3));
        assert!(
            should_bump(false, false, false, false, 2, 3),
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
                                    should_bump(sticky, permasage, permaage, sage, count, limit),
                                    expected
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
}
