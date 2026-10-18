//! Source ban-request review decisions. The caller supplies an authenticated
//! reviewer, a locked request/template and qualified retained ban history.
//! These values describe effects; they never grant permission to apply them.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Duration {
    Warning,
    FiniteSeconds(u64),
    Indefinite,
}

#[derive(Clone, Copy, Debug)]
pub struct Template {
    pub id: u32,
    pub duration: Duration,
    pub global: bool,
    pub save_json_only: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Review {
    pub amended: bool,
    pub requested_warning: bool,
    /// Only an amended request consumes this field. Source units are days.
    pub amended_days: Option<i32>,
    /// Warnings always use the selected template's scope.
    pub amended_global: Option<bool>,
    /// Count only active, unexpired, finite local bans on distinct boards.
    pub active_local_boards: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DuplicateScope {
    None,
    Warnings,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Decision {
    pub duration: Duration,
    /// The caller's `$warn` branch, which also governs duplicate clearing and
    /// blacklist effects. A reconfigured zero-day template can expire at once
    /// without having entered this branch.
    pub warning_action: bool,
    pub global: bool,
    /// For the same private actor and the accepted template, excluding the
    /// primary request. Removing duplicates records no additional outcome.
    pub duplicates: DuplicateScope,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidTemplate,
    InvalidDuration,
    WarningNotPermitted,
}

/// The asynchronous template caller clears the submitted warning flag before
/// applying the selected template. A zero-day template still creates a warning.
pub fn request_warning(
    template_warning: bool,
    can_warn: bool,
    submitted_warning: bool,
    asynchronous_template: bool,
) -> Result<bool, Error> {
    if submitted_warning && !asynchronous_template {
        if !can_warn {
            return Err(Error::WarningNotPermitted);
        }
        return Ok(true);
    }
    Ok(template_warning)
}

pub fn accept(template: Template, review: Review) -> Result<Decision, Error> {
    if template.id == 0 || template.id > i32::MAX as u32 || matches!(template.id, 130 | 131) {
        return Err(Error::InvalidTemplate);
    }
    if matches!(template.duration, Duration::FiniteSeconds(seconds) if seconds == 0 || seconds > 863_913_600)
    {
        return Err(Error::InvalidDuration);
    }
    let amended_days = review.amended.then_some(review.amended_days).flatten();
    if amended_days.is_some_and(|days| !(-1..=1024).contains(&days)) {
        return Err(Error::InvalidDuration);
    }
    let warning = amended_days == Some(0) || (!review.amended && review.requested_warning);
    let duration = if warning {
        Duration::Warning
    } else if let Some(days) = amended_days {
        if days == -1 {
            Duration::Indefinite
        } else {
            Duration::FiniteSeconds(days as u64 * 86_400)
        }
    } else if review.amended && template.duration == Duration::Warning {
        // Amending to a zero-day template without an explicit zero-day length
        // creates a one-day ban in the supplied caller.
        Duration::FiniteSeconds(86_400)
    } else {
        template.duration
    };
    let mut global = if review.amended && !warning {
        review.amended_global.unwrap_or(template.global)
    } else {
        template.global
    };
    if !review.amended && !warning && !global && review.active_local_boards >= 1 {
        global = true;
    }
    let duplicates = if template.save_json_only {
        DuplicateScope::None
    } else if warning {
        DuplicateScope::Warnings
    } else {
        DuplicateScope::All
    };
    Ok(Decision {
        duration,
        warning_action: warning,
        global,
        duplicates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_requests_match_the_complete_isolated_source_caller() {
        let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../tests/fixtures/janitor-request-caller.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 97);
        for (index, case) in cases.iter().enumerate() {
            let input = &case["input"];
            let expected = &case["expected"];
            let result = request_warning(
                input["template_warning"].as_bool().unwrap(),
                input["can_warn"].as_bool().unwrap(),
                input["warning"].as_bool().unwrap(),
                input["async"].as_bool().unwrap(),
            );
            if expected["error"].is_null() {
                assert_eq!(
                    result,
                    Ok(expected["warning_request"].as_bool().unwrap()),
                    "case {index}"
                );
                assert_eq!(expected["requests"], 1, "case {index}");
                assert_eq!(
                    expected["delete_calls"].as_array().unwrap().len(),
                    1,
                    "case {index}"
                );
                assert_eq!(expected["undefined_row"], true, "case {index}");
            } else {
                assert_eq!(result, Err(Error::WarningNotPermitted), "case {index}");
                assert_eq!(expected["requests"], 0, "case {index}");
                assert!(
                    expected["delete_calls"].as_array().unwrap().is_empty(),
                    "case {index}"
                );
            }
            assert!(
                expected["range_calls"].as_array().unwrap().is_empty(),
                "case {index}"
            );
        }
    }

    #[test]
    fn review_matches_isolated_source_decisions() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/ban-request-review.json"))
                .unwrap();
        assert_eq!(cases.len(), 2016);
        for (index, case) in cases.iter().enumerate() {
            let input = &case["input"];
            let expected = &case["expected"];
            let template = Template {
                id: 17,
                duration: match input["template_duration"].as_str().unwrap() {
                    "warning" => Duration::Warning,
                    "finite" => Duration::FiniteSeconds(7 * 86_400),
                    "indefinite" => Duration::Indefinite,
                    _ => unreachable!(),
                },
                global: input["template_global"].as_bool().unwrap(),
                save_json_only: input["json_only"].as_bool().unwrap(),
            };
            let decision = accept(
                template,
                Review {
                    amended: input["amended"].as_bool().unwrap(),
                    requested_warning: input["requested_warning"].as_bool().unwrap(),
                    amended_days: input["days"]
                        .as_i64()
                        .map(|days| i32::try_from(days).unwrap()),
                    amended_global: input["global_override"].as_bool(),
                    active_local_boards: u32::from(input["active_local"].as_bool().unwrap()),
                },
            );
            if expected.get("error").is_some() {
                assert_eq!(decision, Err(Error::InvalidDuration), "case {index}");
                continue;
            }
            let decision = decision.unwrap();
            assert_eq!(
                decision.warning_action,
                expected["warning_action"].as_bool().unwrap(),
                "case {index}"
            );
            assert_eq!(
                decision.global,
                expected["global"].as_bool().unwrap(),
                "case {index}"
            );
            let seconds = match decision.duration {
                Duration::Warning => Some(0),
                Duration::FiniteSeconds(seconds) => Some(seconds),
                Duration::Indefinite => None,
            };
            assert_eq!(seconds, expected["seconds"].as_u64(), "case {index}");
            let duplicates = match decision.duplicates {
                DuplicateScope::None => "none",
                DuplicateScope::Warnings => "warnings",
                DuplicateScope::All => "all",
            };
            assert_eq!(
                duplicates,
                expected["duplicates"].as_str().unwrap(),
                "case {index}"
            );
        }
    }

    #[test]
    fn asynchronous_request_ignores_warning_checkbox_but_preserves_warning_templates() {
        assert_eq!(request_warning(false, false, true, true), Ok(false));
        assert_eq!(request_warning(true, false, true, true), Ok(true));
        assert_eq!(
            request_warning(false, false, true, false),
            Err(Error::WarningNotPermitted)
        );
        assert_eq!(
            request_warning(true, false, true, false),
            Err(Error::WarningNotPermitted)
        );
        assert_eq!(request_warning(false, true, true, false), Ok(true));
    }

    #[test]
    fn invalid_templates_and_amended_duration_are_rejected() {
        let template = Template {
            id: 17,
            duration: Duration::FiniteSeconds(86_400),
            global: false,
            save_json_only: false,
        };
        for id in [0, 130, 131, u32::MAX] {
            assert_eq!(
                accept(Template { id, ..template }, Review::default()),
                Err(Error::InvalidTemplate)
            );
        }
        for days in [i32::MIN, -2, 1025, i32::MAX] {
            assert_eq!(
                accept(
                    template,
                    Review {
                        amended: true,
                        amended_days: Some(days),
                        ..Review::default()
                    }
                ),
                Err(Error::InvalidDuration)
            );
            // The source only reads ban_length inside the amendment branch.
            assert!(
                accept(
                    template,
                    Review {
                        amended_days: Some(days),
                        ..Review::default()
                    }
                )
                .is_ok()
            );
        }
    }
}
