use super::text;
use crate::ValidationError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: u16,
    pub label: String,
    pub short_label: String,
}
pub struct Profile {
    pub categories: Vec<Category>,
    pub subject_limit: usize,
    pub message_limit: usize,
}
impl Profile {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.categories.is_empty()
            || self.categories.len() > 32
            || !(1..=512).contains(&self.subject_limit)
            || !(1..=16_000).contains(&self.message_limit)
        {
            return Err(ValidationError("Invalid feedback profile."));
        }
        let mut ids = std::collections::BTreeSet::new();
        for category in &self.categories {
            if !ids.insert(category.id) {
                return Err(ValidationError("Duplicate feedback category."));
            }
            text(&category.label, 100, false, true)?;
            text(&category.short_label, 32, false, true)?;
        }
        Ok(())
    }
}
pub struct Submission {
    pub category: u16,
    pub subject: String,
    pub message: String,
}
impl Submission {
    pub fn prepare(
        profile: &Profile,
        category: u16,
        subject: &str,
        message: &str,
    ) -> Result<Self, ValidationError> {
        profile.validate()?;
        if !profile.categories.iter().any(|c| c.id == category) {
            return Err(ValidationError("Invalid feedback category."));
        }
        Ok(Self {
            category,
            subject: text(subject, profile.subject_limit, false, true)?,
            message: text(message, profile.message_limit, true, true)?,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Answered,
    Dismissed,
    Draft,
}
impl Status {
    pub fn filter(value: Option<&str>) -> Option<Self> {
        match value {
            None => Some(Self::Pending),
            Some("1") => Some(Self::Answered),
            Some("2") => Some(Self::Dismissed),
            Some("3") => Some(Self::Draft),
            _ => None,
        }
    }
}
/// Source index controls: unanswered rows use can_dismiss; only administrators
/// see approval/dismissal controls for the draft list. The reply form is inside
/// an HTML comment and is not an active source control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewControls {
    pub answer_columns: bool,
    pub dismiss: bool,
    pub approve: bool,
    pub reply_form: bool,
}
pub fn review_controls(status: Status, can_dismiss: bool, admin: bool) -> ReviewControls {
    let pending = status == Status::Pending;
    ReviewControls {
        answer_columns: !pending,
        dismiss: if pending {
            can_dismiss
        } else {
            status == Status::Draft && admin
        },
        approve: status == Status::Draft && admin,
        reply_form: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_category_seeds_require_an_explicit_bounded_profile() {
        let mut profile = Profile {
            categories: vec![Category {
                id: 0,
                label: "Owned category".into(),
                short_label: "Owned".into(),
            }],
            subject_limit: 5,
            message_limit: 10,
        };
        assert!(Submission::prepare(&profile, 0, "hello", "<b>text").is_ok());
        assert!(Submission::prepare(&profile, 1, "hello", "text").is_err());
        assert!(Submission::prepare(&profile, 0, "longer", "text").is_err());
        profile.categories.push(profile.categories[0].clone());
        assert!(profile.validate().is_err());
    }
    #[test]
    fn review_filters_never_activate_the_commented_reply_form() {
        for status in [
            Status::Pending,
            Status::Answered,
            Status::Dismissed,
            Status::Draft,
        ] {
            for admin in [false, true] {
                for can_dismiss in [false, true] {
                    let view = review_controls(status, can_dismiss, admin);
                    assert!(!view.reply_form);
                    assert_eq!(view.approve, status == Status::Draft && admin);
                    assert_eq!(view.answer_columns, status != Status::Pending);
                }
            }
        }
        for invalid in ["0", "4", "01", "draft", ""] {
            assert_eq!(Status::filter(Some(invalid)), None);
        }
    }
}

#[cfg(test)]
mod source_templates {
    use super::*;
    use serde_json::Value;

    #[test]
    fn review_controls_match_active_source_markup_not_commented_forms() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/site-submission-reference.json"
        ))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            if input["template"] != "feedback-review.tpl.php" {
                continue;
            }
            count += 1;
            let filter = input["answered"].as_u64().map(|n| n.to_string());
            let view = review_controls(
                Status::filter(filter.as_deref()).unwrap(),
                input["can_dismiss"].as_bool().unwrap(),
                input["admin"].as_bool().unwrap(),
            );
            let output = &case["output"];
            assert!(output["forms"].as_array().unwrap().is_empty());
            assert!(!view.reply_form);
            let actions = output["actions"].as_array().unwrap();
            let has = |class: &str| {
                actions.iter().any(|a| {
                    a["class"]
                        .as_str()
                        .unwrap()
                        .split_whitespace()
                        .any(|c| c == class)
                })
            };
            let rows = input["mode"] == "index" && !input["empty"].as_bool().unwrap();
            assert_eq!(has("dismiss-link"), rows && view.dismiss);
            assert_eq!(has("approve-link"), rows && view.approve);
        }
        assert_eq!(count, 64);
    }

    #[test]
    fn public_feedback_has_no_identity_field_and_only_index_can_submit() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/site-submission-reference.json"
        ))
        .unwrap();
        let mut count = 0;
        for case in fixture["cases"].as_array().unwrap() {
            if case["input"]["template"] != "feedback.tpl.php" {
                continue;
            }
            count += 1;
            let forms = case["output"]["forms"].as_array().unwrap();
            if case["input"]["mode"] != "index" {
                assert!(forms.is_empty());
                continue;
            }
            assert_eq!(forms.len(), 1);
            assert_eq!(forms[0]["attributes"]["method"], "post");
            assert_eq!(forms[0]["attributes"]["enctype"], "multipart/form-data");
            let names: Vec<_> = forms[0]["fields"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|f| f["name"].as_str())
                .collect();
            assert_eq!(names, ["category", "subject", "message", "action"]);
        }
        assert_eq!(count, 6);
    }
}
