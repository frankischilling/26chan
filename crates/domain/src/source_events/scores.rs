//! Dormant April 2019 scoring rules. Trusted callers must verify identity,
//! challenges and target ownership, then persist the outcome atomically.
//! The historical getter reports zero even when its internal ledger changes.
use crate::{ValidationError, php_scalar};
pub const MAX_LIKES: i64 = 1_048_576;
pub const COOLDOWN_SECONDS: i64 = 120;
pub const POST_POINTS: i64 = 2;
pub const GIVEN_POINTS: i64 = 1;
pub const RECEIVED_POINTS: i64 = 10;

pub fn awards_points(board: &str) -> bool {
    !matches!(board, "b" | "qa" | "s4s" | "bant" | "vip")
}
/// Match the source's scalar coercion and exact echoed field. This echo is
/// not authentication or CSRF protection; native callers need both separately.
pub fn parse_post_id(field: Option<&str>, echo: Option<&str>) -> Result<i64, ValidationError> {
    let (Some(field), Some(echo)) = (field, echo) else {
        return Err(ValidationError("Bad Request."));
    };
    if field.len() > 100 || echo.len() > 100 {
        return Err(ValidationError("Bad Request."));
    }
    let id = php_scalar::integer(field);
    if id == 0 {
        return Err(ValidationError("Bad Post ID."));
    }
    if field != echo {
        return Err(ValidationError("Bad Request."));
    }
    Ok(id)
}
#[derive(Clone, Copy, Debug)]
pub struct Eligibility {
    pub target_exists: bool,
    pub own_identity: bool,
    pub own_peer: bool,
    pub duplicate: bool,
    pub range_banned: bool,
    pub challenge_bypass: bool,
    pub last_vote_at: Option<i64>,
    pub now: i64,
}
impl Eligibility {
    pub fn check(self) -> Result<(), ValidationError> {
        if !self.target_exists {
            return Err(ValidationError("You can't like this post."));
        }
        if self.own_identity || self.own_peer {
            return Err(ValidationError("You can't like your own posts."));
        }
        if self.duplicate {
            return Err(ValidationError("You already like this post."));
        }
        if self.now < 0 {
            return Err(ValidationError("Invalid event time"));
        }
        if self
            .last_vote_at
            .is_some_and(|last| last > self.now.saturating_sub(COOLDOWN_SECONDS))
            || (self.range_banned && !self.challenge_bypass)
        {
            return Err(ValidationError(
                "You have to wait a while before liking this post.",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ledger {
    pub likes: i64,
    pub giver: i64,
    pub receiver: i64,
}
impl Ledger {
    /// A successful outcome includes both scores and the counter. Callers must
    /// not store a duplicate-vote log or one score if any check fails.
    pub fn like(self, board: &str) -> Result<Self, ValidationError> {
        let likes = self
            .likes
            .checked_add(1)
            .filter(|v| self.likes >= 0 && *v < MAX_LIKES)
            .ok_or(ValidationError("You can't like this post anymore."))?;
        let (giver, receiver) = if awards_points(board) {
            (
                self.giver.checked_add(GIVEN_POINTS),
                self.receiver.checked_add(RECEIVED_POINTS),
            )
        } else {
            (Some(self.giver), Some(self.receiver))
        };
        Ok(Self {
            likes,
            giver: giver.ok_or(ValidationError("Invalid event score"))?,
            receiver: receiver.ok_or(ValidationError("Invalid event score"))?,
        })
    }
}
pub fn post_score(board: &str, score: i64) -> Result<i64, ValidationError> {
    if awards_points(board) {
        score
            .checked_add(POST_POINTS)
            .ok_or(ValidationError("Invalid event score"))
    } else {
        Ok(score)
    }
}
pub fn reported_score() -> i64 {
    0
}

/// Bind a challenge to one score run as well as its normal action/board/post
/// scope. Resetting an event changes this nonce even for the same posting-form nonce.
/// This is domain separation, not a replacement for validating the browser's CSRF cookie.
pub fn form_nonce(run_id: i64, csrf: &str) -> Result<String, ValidationError> {
    if run_id <= 0
        || csrf.len() != 64
        || !csrf
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ValidationError("Invalid event form"));
    }
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    digest.update(b"26chan-score-form-v1\0");
    digest.update(&run_id.to_be_bytes());
    digest.update(csrf.as_bytes());
    Ok(digest
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn outcomes_match_executed_source_controller_and_post_hook() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/event-score-reference.json"
        ))
        .unwrap();
        for c in fixture["cases"].as_array().unwrap() {
            let boolean = |name: &str| c[name].as_bool().unwrap();
            let board = c["board"].as_str().unwrap();
            let pass_actor = boolean("pass") && boolean("bypass");
            let actor = if pass_actor {
                "owned-pass"
            } else {
                "192.0.2.1"
            };
            if c["operation"] == "post" {
                assert_eq!(
                    post_score(board, 100).unwrap(),
                    c["expected"]["scores"][actor]
                );
                continue;
            }
            let result = (|| -> Result<Ledger, ValidationError> {
                parse_post_id(c["fields"]["post_id"].as_str(), c["cookie"].as_str())?;
                if (boolean("pass") || !boolean("known"))
                    && !boolean("bypass")
                    && !boolean("captcha_ok")
                {
                    // Owned oracle stand-in, not a public challenge message.
                    return Err(ValidationError("Owned CAPTCHA refused."));
                }
                Eligibility {
                    target_exists: boolean("target"),
                    own_identity: pass_actor && boolean("self_pass"),
                    own_peer: boolean("self_peer"),
                    duplicate: boolean("duplicate"),
                    range_banned: boolean("range_banned"),
                    challenge_bypass: boolean("bypass"),
                    last_vote_at: Some(c["last"].as_i64().unwrap()),
                    now: c["now"].as_i64().unwrap(),
                }
                .check()?;
                if boolean("disappeared") {
                    return Err(ValidationError("This post doesn't exist anymore."));
                }
                if boolean("op") || boolean("archived") {
                    return Err(ValidationError("You can't like this post."));
                }
                Ledger {
                    likes: c["likes"].as_i64().unwrap(),
                    giver: 100,
                    receiver: if boolean("self_pass") { 100 } else { 200 },
                }
                .like(board)
            })();
            match result {
                Ok(next) => {
                    let expected = format!(
                        "1\n{}\n{}\n{}",
                        next.likes,
                        reported_score(),
                        super::super::PERKS.join(" ")
                    );
                    assert_eq!(expected, c["expected"]["response"], "{c}");
                    assert_eq!(next.giver, c["expected"]["scores"][actor], "{c}");
                    assert_eq!(
                        next.receiver,
                        c["expected"]["scores"][if boolean("self_pass") {
                            "owned-pass"
                        } else {
                            "owned-target"
                        }],
                        "{c}"
                    );
                }
                Err(error) => {
                    assert_eq!(format!("0\n{}", error.0), c["expected"]["response"], "{c}")
                }
            }
        }
    }
    #[test]
    fn form_nonce_binds_event_reset_and_csrf_without_adding_identity() {
        let csrf = "ab".repeat(32);
        let first = form_nonce(1, &csrf).unwrap();
        assert_eq!(first.len(), 64);
        assert_ne!(first, form_nonce(2, &csrf).unwrap());
        assert_ne!(first, form_nonce(1, &"cd".repeat(32)).unwrap());
        assert_ne!(first, csrf);
        assert!(form_nonce(0, &csrf).is_err());
        assert!(form_nonce(1, &"AB".repeat(32)).is_err());
        assert!(form_nonce(1, &"z".repeat(64)).is_err());
    }
    #[test]
    fn overflow_never_produces_a_partial_score_or_counter() {
        let ledger = Ledger {
            likes: 0,
            giver: i64::MAX,
            receiver: 1,
        };
        assert!(ledger.like("g").is_err());
        assert_eq!(ledger.likes, 0);
        assert_eq!(ledger.like("b").unwrap(), Ledger { likes: 1, ..ledger });
        assert!(post_score("g", i64::MAX).is_err());
        assert!(
            Ledger {
                likes: MAX_LIKES - 1,
                giver: 0,
                receiver: 0
            }
            .like("g")
            .is_err()
        );
        assert!(parse_post_id(Some(&"1".repeat(101)), Some(&"1".repeat(101))).is_err());
    }
}
