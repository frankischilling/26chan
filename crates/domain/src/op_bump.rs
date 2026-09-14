/// Source OP rules set sage, with strict comparisons and equality eligible.
/// The latest surviving own reply is ordered by post number, not timestamp.
pub fn limited(
    enabled: bool,
    request_seconds: i64,
    op_seconds: i64,
    latest_own_seconds: Option<i64>,
    initial_seconds: u32,
    repeat_seconds: u32,
) -> bool {
    enabled
        && (i128::from(op_seconds) > i128::from(request_seconds) - i128::from(initial_seconds)
            || latest_own_seconds.is_some_and(|latest| {
                i128::from(latest) > i128::from(request_seconds) - i128::from(repeat_seconds)
            }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_and_repeat_windows_have_strict_boundaries() {
        for initial in [0, 120, 300, 600, 900, u32::MAX] {
            let cutoff = 1000 + i64::from(initial);
            assert!(limited(true, cutoff - 1, 1000, None, initial, 300));
            assert!(!limited(true, cutoff, 1000, None, initial, 300));
            assert!(!limited(true, cutoff + 1, 1000, None, initial, 300));
        }
        for repeat in [0, 300, u32::MAX] {
            let cutoff = 2000 + i64::from(repeat);
            assert!(limited(true, cutoff - 1, 1000, Some(2000), 0, repeat));
            assert!(!limited(true, cutoff, 1000, Some(2000), 0, repeat));
            assert!(!limited(true, cutoff + 1, 1000, Some(2000), 0, repeat));
        }
        assert!(!limited(
            false,
            i64::MIN,
            i64::MAX,
            Some(i64::MAX),
            u32::MAX,
            u32::MAX
        ));
        assert!(limited(true, i64::MIN, i64::MIN, None, 1, 0));
        assert!(!limited(
            true,
            i64::MAX,
            i64::MIN,
            Some(i64::MIN),
            u32::MAX,
            u32::MAX
        ));
    }
}
