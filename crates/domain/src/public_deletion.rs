//! Public deletion gates from imgboard.php:2464–2530. Authority itself is
//! established by the caller; staff and automatic deletions do not use these.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    no_op: bool,
    no_reply: bool,
    known_min_seconds: u32,
    unknown_min_seconds: u32,
    max_seconds: u32,
}

impl Policy {
    pub fn new(no_op: bool, no_reply: bool, known: i32, unknown: i32, max: i32) -> Option<Self> {
        if !(0..=86400).contains(&known)
            || !(known..=86400).contains(&unknown)
            || !(1..=86400).contains(&max)
            || unknown >= max
        {
            return None;
        }
        Some(Self {
            no_op,
            no_reply,
            known_min_seconds: known as u32,
            unknown_min_seconds: unknown as u32,
            max_seconds: max as u32,
        })
    }

    /// Source checks the upper age and board gate before password authority.
    pub fn before_authority(self, op: bool, age: u64) -> Result<(), Rejection> {
        if age >= u64::from(self.max_seconds) {
            return Err(Rejection::TooOld);
        }
        if if op { self.no_op } else { self.no_reply } {
            return Err(Rejection::Forbidden);
        }
        Ok(())
    }

    pub fn after_authority(
        self,
        target: Target,
        age: u64,
        network_age: u64,
    ) -> Result<(), Rejection> {
        if target.archived {
            return Err(Rejection::BadPassword);
        }
        if target.sticky || (target.op && (target.vg || target.staff_reply)) {
            return Err(Rejection::Forbidden);
        }
        let minimum = if network_age >= 900 {
            self.known_min_seconds
        } else {
            self.unknown_min_seconds
        };
        if age < u64::from(minimum) {
            return Err(Rejection::TooYoung);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Target {
    pub op: bool,
    pub archived: bool,
    pub sticky: bool,
    pub vg: bool,
    pub staff_reply: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    TooOld,
    Forbidden,
    TooYoung,
    BadPassword,
}
impl Rejection {
    pub fn message(self) -> &'static str {
        match self {
            Self::TooOld => "Error: You cannot delete a post this old.",
            Self::Forbidden => "Error: You cannot delete this post.",
            Self::TooYoung => "Error: You must wait longer before deleting this post.",
            Self::BadPassword => "Error: Password incorrect.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy::new(false, false, 60, 600, 1800).unwrap()
    }
    #[test]
    fn source_age_boundaries_and_network_threshold() {
        for (age, network, expected) in [
            (59, 900, false),
            (60, 900, true),
            (599, 899, false),
            (600, 899, true),
            (60, 899, false),
            (60, 900, true),
            (1799, 900, true),
        ] {
            assert_eq!(
                policy()
                    .after_authority(Target::default(), age, network)
                    .is_ok(),
                expected
            );
        }
        assert!(policy().before_authority(false, 1799).is_ok());
        assert_eq!(
            policy().before_authority(false, 1800),
            Err(Rejection::TooOld)
        );
    }
    #[test]
    fn source_gate_precedence_and_protection() {
        let mut p = policy();
        p.no_op = true;
        p.no_reply = true;
        assert_eq!(p.before_authority(true, 1800), Err(Rejection::TooOld));
        assert_eq!(p.before_authority(true, 600), Err(Rejection::Forbidden));
        assert_eq!(p.before_authority(false, 600), Err(Rejection::Forbidden));
        for target in [
            Target {
                sticky: true,
                ..Target::default()
            },
            Target {
                op: true,
                vg: true,
                ..Target::default()
            },
            Target {
                op: true,
                staff_reply: true,
                ..Target::default()
            },
        ] {
            assert_eq!(
                policy().after_authority(target, 0, 0),
                Err(Rejection::Forbidden)
            );
        }
        assert_eq!(
            policy().after_authority(
                Target {
                    archived: true,
                    sticky: true,
                    ..Target::default()
                },
                0,
                0
            ),
            Err(Rejection::BadPassword)
        );
        assert!(
            policy()
                .after_authority(
                    Target {
                        vg: true,
                        staff_reply: true,
                        ..Target::default()
                    },
                    600,
                    0
                )
                .is_ok()
        );
    }
    #[test]
    fn bounded_operator_policy() {
        for (known, unknown, max) in [
            (-1, 600, 1800),
            (60, 59, 1800),
            (60, 1800, 1800),
            (0, 0, 0),
            (0, 0, 86401),
        ] {
            assert!(Policy::new(false, false, known, unknown, max).is_none());
        }
        assert!(Policy::new(false, false, 0, 0, 1800).is_some());
    }
}
