use super::Random;

/// Native entropy for the source's two bounded draw ranges. It shares the
/// rejection-sampling helper used by dice and fortunes, with no modulo bias.
/// It does not reproduce PHP's global PRNG seed or cross-request sequence.
pub struct NativeRandom(ring::rand::SystemRandom);
impl Default for NativeRandom {
    fn default() -> Self {
        Self(ring::rand::SystemRandom::new())
    }
}
impl Random for NativeRandom {
    fn draw(&mut self, upper_exclusive: u8) -> Option<u8> {
        draw_with(upper_exclusive, |upper| {
            crate::posting_randomizers::random_below(&self.0, upper).ok()
        })
    }
}
fn draw_with(upper: u8, generate: impl FnOnce(u32) -> Option<u32>) -> Option<u8> {
    if !matches!(upper, 2 | 10) {
        return None;
    }
    let value = generate(u32::from(upper))?;
    (value < u32::from(upper)).then_some(value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_or_invalid_entropy_never_becomes_a_favorable_draw() {
        for upper in [2, 10] {
            assert_eq!(draw_with(upper, |_| None), None);
            assert_eq!(draw_with(upper, |_| Some(u32::from(upper))), None);
            assert_eq!(draw_with(upper, |_| Some(u32::MAX)), None);
            assert_eq!(draw_with(upper, |_| Some(0)), Some(0));
            assert_eq!(
                draw_with(upper, |_| Some(u32::from(upper - 1))),
                Some(upper - 1)
            );
        }
        for upper in [0, 1, 3, 255] {
            assert_eq!(
                draw_with(upper, |_| panic!("Unsupported range consumed entropy")),
                None
            );
        }
    }
    #[test]
    fn native_draws_accept_only_the_source_ranges() {
        let mut random = NativeRandom::default();
        for upper in 0..=u8::MAX {
            if matches!(upper, 2 | 10) {
                for _ in 0..64 {
                    assert!(random.draw(upper).is_some_and(|value| value < upper));
                }
            } else {
                assert_eq!(random.draw(upper), None);
            }
        }
    }
}
