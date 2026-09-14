/// Source limit indicators use surviving replies and exclude sticky threads.
pub fn limited(sticky: bool, replies: u64, limit: u32) -> bool {
    !sticky && replies >= u64::from(limit)
}

/// The source checks the count after inserting the incoming reply.
/// Permaage/permasage and board age/self-bump policies are separate rules.
pub fn should_bump(sticky: bool, sage: bool, replies_after: u64, limit: u32) -> bool {
    !sticky && !sage && !limited(sticky, replies_after, limit)
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
            assert_eq!(should_bump(false, false, count, limit), bump);
            assert_eq!(limited(false, count, limit), !bump);
            assert!(!should_bump(false, true, count, limit));
            assert!(!should_bump(true, false, count, limit));
            assert!(!limited(true, count, limit));
        }
        assert!(!should_bump(false, false, 3, 3));
        assert!(
            should_bump(false, false, 2, 3),
            "deleted replies no longer count"
        );
        assert!(limited(false, u64::MAX, u32::MAX));
    }
}
