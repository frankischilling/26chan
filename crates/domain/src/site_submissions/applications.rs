use super::{ContactEmail, text};
use crate::{BoardSlug, ValidationError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormState {
    Closed,
    DesktopRequired,
    AuthenticateEmail,
    New,
    Edit,
}
pub fn form_state(open: bool, desktop: bool, need_email: bool, existing: bool) -> FormState {
    if !open {
        FormState::Closed
    } else if !desktop {
        FormState::DesktopRequired
    } else if need_email {
        FormState::AuthenticateEmail
    } else if existing {
        FormState::Edit
    } else {
        FormState::New
    }
}
pub const TIMEZONES: std::ops::RangeInclusive<i16> = -12..=13;
pub struct Profile {
    pub boards: Vec<String>,
    pub questions: [String; 4],
    pub field_limit: usize,
    pub answer_limit: usize,
}
impl Profile {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.boards.is_empty()
            || self.boards.len() > 128
            || !(1..=512).contains(&self.field_limit)
            || !(1..=16_000).contains(&self.answer_limit)
        {
            return Err(ValidationError("Invalid application profile."));
        }
        let mut boards = std::collections::BTreeSet::new();
        for board in &self.boards {
            BoardSlug::parse(board)?;
            if !boards.insert(board) {
                return Err(ValidationError("Duplicate application board."));
            }
        }
        for question in &self.questions {
            text(question, 1000, false, true)?;
        }
        Ok(())
    }
}
pub struct Input<'a> {
    pub firstname: &'a str,
    pub handle: &'a str,
    pub email: &'a str,
    pub age: u16,
    pub timezone: i16,
    pub hours: u8,
    pub times: &'a str,
    pub board1: &'a str,
    pub board2: &'a str,
    pub answers: [&'a str; 4],
}
/// Identity is deliberately separate from the blind review fields.
pub struct Identity {
    pub firstname: String,
    pub handle: String,
    pub email: ContactEmail,
}
pub struct Answers {
    pub age: u16,
    pub timezone: i16,
    pub hours: u8,
    pub times: String,
    pub board1: String,
    pub board2: Option<String>,
    pub answers: [String; 4],
}
pub struct Submission {
    pub identity: Identity,
    pub review: Answers,
}
impl Submission {
    pub fn prepare(profile: &Profile, input: Input<'_>) -> Result<Self, ValidationError> {
        profile.validate()?;
        if !(1..=150).contains(&input.age)
            || !TIMEZONES.contains(&input.timezone)
            || !(1..=24).contains(&input.hours)
            || !profile.boards.iter().any(|b| b == input.board1)
            || !input.board2.is_empty() && !profile.boards.iter().any(|b| b == input.board2)
        {
            return Err(ValidationError("Invalid application selection."));
        }
        let mut answers = Vec::with_capacity(4);
        for value in input.answers {
            answers.push(text(value, profile.answer_limit, true, true)?);
        }
        Ok(Self {
            identity: Identity {
                firstname: text(input.firstname, profile.field_limit, false, true)?,
                handle: text(input.handle, profile.field_limit, false, true)?,
                email: ContactEmail::parse(&text(input.email, profile.field_limit, false, true)?)?,
            },
            review: Answers {
                age: input.age,
                timezone: input.timezone,
                hours: input.hours,
                times: text(input.times, profile.field_limit, false, true)?,
                board1: input.board1.into(),
                board2: (!input.board2.is_empty()).then(|| input.board2.into()),
                answers: answers
                    .try_into()
                    .map_err(|_| ValidationError("Invalid application answers."))?,
            },
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_and_desktop_gates_precede_edit_authentication() {
        for desktop in [false, true] {
            for auth in [false, true] {
                for old in [false, true] {
                    assert_eq!(form_state(false, desktop, auth, old), FormState::Closed);
                }
            }
        }
        assert_eq!(
            form_state(true, false, true, true),
            FormState::DesktopRequired
        );
        assert_eq!(
            form_state(true, true, true, true),
            FormState::AuthenticateEmail
        );
        assert_eq!(form_state(true, true, false, false), FormState::New);
        assert_eq!(form_state(true, true, false, true), FormState::Edit);
        assert_eq!(TIMEZONES.clone().count(), 26);
        assert!(!TIMEZONES.contains(&14));
    }
    #[test]
    fn synthetic_application_keeps_identity_outside_review_fields() {
        let profile = Profile {
            boards: vec!["g".into()],
            questions: std::array::from_fn(|i| format!("Owned question {i}")),
            field_limit: 150,
            answer_limit: 1000,
        };
        let submitted = Submission::prepare(
            &profile,
            Input {
                firstname: "Owned",
                handle: "Owned handle",
                email: "owned@example.org",
                age: 25,
                timezone: 13,
                hours: 24,
                times: "16:00-18:00",
                board1: "g",
                board2: "",
                answers: ["One", "Two", "Three", "Four"],
            },
        )
        .unwrap();
        assert_eq!(submitted.identity.email.recipient(), "owned@example.org");
        assert_eq!(submitted.review.board1, "g");
        assert_eq!(submitted.review.answers[3], "Four");
        assert!(submitted.review.board2.is_none());
    }
}

#[cfg(test)]
mod source_templates {
    use super::*;
    use serde_json::Value;

    #[test]
    fn all_application_gates_and_fields_match_rendered_source_templates() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/site-submission-reference.json"
        ))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            if input["template"] != "janitorapp.tpl.php" {
                continue;
            }
            count += 1;
            let output = &case["output"];
            let state = form_state(
                input["open"].as_bool().unwrap(),
                input["desktop"].as_bool().unwrap(),
                input["need_email"].as_bool().unwrap(),
                input["existing"].as_bool().unwrap(),
            );
            let forms = output["forms"].as_array().unwrap();
            assert_eq!(output["closed"], state == FormState::Closed);
            assert_eq!(
                output["desktop_required"],
                state == FormState::DesktopRequired
            );
            if matches!(state, FormState::Closed | FormState::DesktopRequired) {
                assert!(forms.is_empty());
                continue;
            }
            assert_eq!(forms.len(), 1);
            let fields = forms[0]["fields"].as_array().unwrap();
            let names: Vec<_> = fields.iter().filter_map(|f| f["name"].as_str()).collect();
            if state == FormState::AuthenticateEmail {
                assert_eq!(names, ["auth_email"]);
                continue;
            }
            let mut expected = vec![
                "firstname",
                "handle",
                "email",
                "age",
                "tz",
                "hours",
                "times",
                "board1",
                "board2",
                "q1",
                "q2",
                "q3",
                "q4",
                "_cf_fuid",
            ];
            if state == FormState::Edit {
                expected.extend(["id", "auth_email"]);
            }
            expected.push("action");
            assert_eq!(names, expected);
            let field = |name: &str| fields.iter().find(|f| f["name"] == name).unwrap();
            let zones: Vec<_> = field("tz")["options"]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| o["value"].as_str().unwrap().parse::<i16>().unwrap())
                .collect();
            assert_eq!(zones, TIMEZONES.collect::<Vec<_>>());
            assert_eq!(
                field("tz").get("data-edit").is_some(),
                state == FormState::Edit
            );
            assert_eq!(field("age")["min"], "1");
            assert_eq!(field("hours")["min"], "1");
            assert_eq!(field("hours")["max"], "24");
            assert!(field("board1").get("required").is_some());
            assert!(field("board2").get("required").is_none());
            for name in ["firstname", "handle", "email", "times"] {
                assert_eq!(field(name)["maxlength"], "150");
            }
            for name in ["q1", "q2", "q3", "q4"] {
                assert_eq!(field(name)["maxlength"], "2000");
                assert!(field(name).get("required").is_some());
            }
        }
        assert_eq!(count, 32);
    }
}
