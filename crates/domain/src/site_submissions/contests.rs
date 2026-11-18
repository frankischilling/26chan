use crate::ValidationError;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    BoardBanner,
    Vtuber,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaType {
    Jpeg,
    Png,
    Gif,
}
/// Dimensions and bytes come from controlled media processing, not a form or
/// a client-supplied MIME type. This model does not approve an upload.
/// The missing controller leaves KB/MB ambiguous; binary units below are a
/// native policy, not a recovered byte-for-byte historical boundary.
pub fn check_image(
    kind: Kind,
    media: MediaType,
    width: u32,
    height: u32,
    bytes: u64,
) -> Result<(), ValidationError> {
    let accepted = width > 0
        && height > 0
        && bytes > 0
        && match kind {
            Kind::BoardBanner => width == 468 && height == 60 && bytes < 150 * 1024,
            Kind::Vtuber => {
                width <= 2048
                    && height <= 2048
                    && bytes <= 5 * 1024 * 1024
                    && media != MediaType::Gif
            }
        };
    accepted.then_some(()).ok_or(ValidationError(
        "Image does not meet this contest's requirements.",
    ))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Closed,
    Submission,
    Review,
    Voting,
    Results,
}
impl Phase {
    pub fn can_submit(self) -> bool {
        self == Self::Submission
    }
    pub fn can_delete(self) -> bool {
        self == Self::Submission
    }
    pub fn can_vote(self) -> bool {
        self == Self::Voting
    }
}
/// The Vtuber template suppresses its entire listing outside voting, even
/// though it contains an unreachable score column. Board banners show results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listing {
    pub visible: bool,
    pub vote: bool,
    pub score: bool,
}
pub fn listing(kind: Kind, votable: bool, has_items: bool) -> Listing {
    let visible = has_items && (kind == Kind::BoardBanner || votable);
    Listing {
        visible,
        vote: visible && votable,
        score: visible && !votable,
    }
}
/// The backend combines this visible scope with the campaign and verified
/// private Pass identity. Neither a presentation cookie nor a public year votes.
pub fn voting_scope(kind: Kind, board: &str) -> Result<&str, ValidationError> {
    match kind {
        Kind::BoardBanner => {
            crate::BoardSlug::parse(board)?;
            Ok(board)
        }
        Kind::Vtuber if board == "all" => Ok("all"),
        _ => Err(ValidationError("Invalid contest board.")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_dimensions_formats_and_native_byte_limits_are_distinct() {
        for media in [MediaType::Jpeg, MediaType::Png, MediaType::Gif] {
            assert!(check_image(Kind::BoardBanner, media, 468, 60, 150 * 1024 - 1).is_ok());
            assert!(check_image(Kind::BoardBanner, media, 468, 60, 150 * 1024).is_err());
            assert!(check_image(Kind::BoardBanner, media, 469, 60, 1).is_err());
        }
        for media in [MediaType::Jpeg, MediaType::Png] {
            assert!(check_image(Kind::Vtuber, media, 2048, 2048, 5 * 1024 * 1024).is_ok());
            assert!(check_image(Kind::Vtuber, media, 2049, 2048, 1).is_err());
            assert!(check_image(Kind::Vtuber, media, 2048, 2048, 5 * 1024 * 1024 + 1).is_err());
        }
        assert!(check_image(Kind::Vtuber, MediaType::Gif, 10, 10, 100).is_err());
    }
    #[test]
    fn phase_and_membership_scope_do_not_grant_submission_authority() {
        for phase in [
            Phase::Closed,
            Phase::Submission,
            Phase::Review,
            Phase::Voting,
            Phase::Results,
        ] {
            assert_eq!(phase.can_submit(), phase.can_delete());
            assert_eq!(phase.can_vote(), phase == Phase::Voting);
        }
        assert_eq!(voting_scope(Kind::BoardBanner, "g").unwrap(), "g");
        assert_eq!(voting_scope(Kind::Vtuber, "all").unwrap(), "all");
        assert!(voting_scope(Kind::Vtuber, "g").is_err());
    }
}

#[cfg(test)]
mod source_templates {
    use super::*;
    use serde_json::Value;
    #[test]
    fn contest_listing_visibility_matches_reachable_source_markup() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/site-submission-reference.json"
        ))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            let kind = match input["template"].as_str().unwrap() {
                "banner-contest.tpl.php" => Kind::BoardBanner,
                "banner-contest-vtube.tpl.php" => Kind::Vtuber,
                _ => continue,
            };
            count += 1;
            let output = &case["output"];
            let has_listing = !input["board"].is_null();
            let view = listing(
                kind,
                input["votable"].as_bool().unwrap(),
                !input["empty"].as_bool().unwrap(),
            );
            assert_eq!(
                !output["vote_buttons"].as_array().unwrap().is_empty(),
                has_listing && view.vote
            );
            assert_eq!(
                output["score_columns"].as_u64().unwrap() > 0,
                has_listing && view.score
            );
            let forms = output["forms"].as_array().unwrap();
            assert_eq!(
                forms.len(),
                usize::from(input["submitting"].as_bool().unwrap())
            );
            if let Some(form) = forms.first() {
                let fields = form["fields"].as_array().unwrap();
                let field = |name: &str| fields.iter().find(|f| f["name"] == name).unwrap();
                for name in ["author", "email"] {
                    assert_eq!(field(name)["maxlength"], "150");
                    assert!(field(name).get("required").is_none());
                }
                assert!(field("file").get("required").is_some());
                if kind == Kind::Vtuber {
                    assert_eq!(field("board")["type"], "hidden");
                    assert_eq!(field("board")["value"], "all");
                } else {
                    assert_eq!(field("board")["tag"], "select");
                    assert!(field("board").get("required").is_some());
                }
            }
        }
        assert_eq!(count, 32);
    }
}

/// Explicit native campaign configuration. Original schedules and controllers
/// are missing, so opening a workflow requires operator-supplied dates/boards.
pub struct Campaign {
    pub id: i64,
    pub kind: Kind,
    pub boards: Vec<String>,
    pub submission_starts: i64,
    pub submission_ends: i64,
    pub voting_starts: i64,
    pub voting_ends: i64,
    pub capacity: u32,
    pub enabled: bool,
}
impl Campaign {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id <= 0
            || self.submission_starts < 0
            || self.submission_ends <= self.submission_starts
            || self.voting_starts < self.submission_ends
            || self.voting_ends <= self.voting_starts
            || self.voting_ends > 253_402_300_799
            || !(1..=100_000).contains(&self.capacity)
            || self.boards.is_empty()
            || self.boards.len() > 128
        {
            return Err(ValidationError("Invalid contest campaign."));
        }
        let mut seen = std::collections::BTreeSet::new();
        for board in &self.boards {
            voting_scope(self.kind, board)?;
            if !seen.insert(board) || self.kind == Kind::BoardBanner && board == "all" {
                return Err(ValidationError("Invalid contest board."));
            }
        }
        if self.kind == Kind::Vtuber && self.boards != ["all"] {
            return Err(ValidationError("Invalid contest board."));
        }
        Ok(())
    }
    /// The supplied time must be the server/database time, never a form value.
    pub fn phase(&self, now: i64) -> Result<Phase, ValidationError> {
        self.validate()?;
        Ok(if !self.enabled || now < self.submission_starts {
            Phase::Closed
        } else if now < self.submission_ends {
            Phase::Submission
        } else if now < self.voting_starts {
            Phase::Review
        } else if now < self.voting_ends {
            Phase::Voting
        } else {
            Phase::Results
        })
    }
}

/// The public credit label and private contact address are distinct fields.
/// Never serialize this whole structure into a public listing.
pub struct Entry {
    pub author: String,
    pub email: Option<super::ContactEmail>,
    pub board: String,
}
impl Entry {
    pub fn prepare(
        campaign: &Campaign,
        author: &str,
        email: &str,
        board: &str,
    ) -> Result<Self, ValidationError> {
        campaign.validate()?;
        if !campaign.boards.iter().any(|allowed| allowed == board) {
            return Err(ValidationError("Invalid contest board."));
        }
        let email = super::text(email, 150, false, false)?;
        Ok(Self {
            author: super::text(author, 150, false, false)?,
            email: if email.trim().is_empty() {
                None
            } else {
                Some(super::ContactEmail::parse(&email)?)
            },
            board: board.into(),
        })
    }
}

#[cfg(test)]
mod campaign_tests {
    use super::*;
    fn campaign() -> Campaign {
        Campaign {
            id: 1,
            kind: Kind::BoardBanner,
            boards: vec!["g".into()],
            submission_starts: 100,
            submission_ends: 200,
            voting_starts: 250,
            voting_ends: 300,
            capacity: 100,
            enabled: true,
        }
    }
    #[test]
    fn campaign_windows_are_half_open_and_never_implicitly_open() {
        let mut c = campaign();
        for (now, phase) in [
            (99, Phase::Closed),
            (100, Phase::Submission),
            (199, Phase::Submission),
            (200, Phase::Review),
            (249, Phase::Review),
            (250, Phase::Voting),
            (299, Phase::Voting),
            (300, Phase::Results),
        ] {
            assert_eq!(c.phase(now).unwrap(), phase);
        }
        c.enabled = false;
        assert_eq!(c.phase(150).unwrap(), Phase::Closed);
        c.voting_starts = 199;
        assert!(c.validate().is_err());
        c.voting_starts = 200;
        assert!(c.validate().is_ok());
        c.boards = vec!["all".into()];
        assert!(c.validate().is_err());
        c.kind = Kind::Vtuber;
        assert!(c.validate().is_ok());
        c.boards.push("all".into());
        assert!(c.validate().is_err());
    }
    #[test]
    fn optional_contact_fields_do_not_authorize_or_publish_an_entry() {
        let c = campaign();
        let anonymous = Entry::prepare(&c, "", "", "g").unwrap();
        assert!(anonymous.email.is_none());
        assert!(anonymous.author.is_empty());
        assert!(Entry::prepare(&c, "Owned", "alias+contest@example.org", "g").is_ok());
        assert!(Entry::prepare(&c, "Owned", "", "all").is_err());
        assert!(Entry::prepare(&c, &"x".repeat(151), "", "g").is_err());
        assert!(Entry::prepare(&c, "Owned", "not an email", "g").is_err());
        assert!(Entry::prepare(&c, "Owned", "\r\n", "g").is_err());
    }
}
