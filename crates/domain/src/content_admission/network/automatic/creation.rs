//! Plans derivative events after an authorized non-warning, global-rule template action.
//! The target evidence must come from the stored post, never moderator fields.
use super::{Event, MAX_EVENTS, browser_valid};
use crate::{BoardSlug, content_admission::AdmissionError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hint,
    Range,
}

pub struct HistoryEvent {
    pub kind: Kind,
    pub event: Event,
}

#[derive(Clone, Copy)]
pub enum Origin {
    AppliedBan,
    BanRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Unchanged,
    RecordHint,
    RecordRange,
}

pub struct Target<'a> {
    pub board: &'a str,
    pub parent: i64,
    pub prefix: &'a [u8; 32],
    pub browser: &'a str,
}

pub struct History {
    events: Vec<HistoryEvent>,
}
impl History {
    pub fn compile(events: Vec<HistoryEvent>) -> Result<Self, AdmissionError> {
        let mut ids = std::collections::HashSet::new();
        if events.len() > MAX_EVENTS
            || events.iter().any(|entry| {
                let event = &entry.event;
                event.id < 1
                    || !ids.insert(event.id)
                    || BoardSlug::parse(&event.board).is_err()
                    || event.parent < 0
                    || !browser_valid(&event.browser)
                    || !event.browser.starts_with('1')
            })
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        Ok(Self { events })
    }

    pub fn plan(
        &self,
        target: Target<'_>,
        origin: Origin,
        now: u64,
    ) -> Result<Action, AdmissionError> {
        if BoardSlug::parse(target.board).is_err()
            || target.parent < 0
            || !browser_valid(target.browser)
            || !target.browser.starts_with('1')
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        let same = |entry: &&HistoryEvent| {
            let event = &entry.event;
            event.board == target.board
                && event.parent == target.parent
                && &event.prefix == target.prefix
                && event.browser == target.browser
        };
        // The source prunes hints strictly older than one hour before counting.
        // COUNT(DISTINCT ip)>0 does not exclude the current address: any prior
        // matching hint suffices, including another request for the same IP.
        let previous_hint = self.events.iter().filter(same).any(|entry| {
            entry.kind == Kind::Hint && u128::from(entry.event.created_at) + 3600 >= u128::from(now)
        });
        if matches!(origin, Origin::BanRequest) && !previous_hint {
            return Ok(Action::RecordHint);
        }
        // Unlike hint retention, the duplicate-range window is strict.
        let recent_range = self.events.iter().filter(same).any(|entry| {
            entry.kind == Kind::Range && u128::from(entry.event.created_at) + 3600 > u128::from(now)
        });
        Ok(if recent_range {
            Action::Unchanged
        } else {
            Action::RecordRange
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: i64, kind: Kind, created_at: u64) -> HistoryEvent {
        HistoryEvent {
            kind,
            event: Event {
                id,
                board: "g".into(),
                parent: 123,
                prefix: [1; 32],
                browser: "1deadbeef".into(),
                created_at,
            },
        }
    }
    fn target() -> Target<'static> {
        Target {
            board: "g",
            parent: 123,
            prefix: &[1; 32],
            browser: "1deadbeef",
        }
    }

    #[test]
    fn direct_bans_and_requests_preserve_distinct_hour_boundaries() {
        for age in [0, 3599, 3600, 3601] {
            let hints = History::compile(vec![event(1, Kind::Hint, 10_000 - age)]).unwrap();
            assert_eq!(
                hints.plan(target(), Origin::BanRequest, 10_000).unwrap(),
                if age <= 3600 {
                    Action::RecordRange
                } else {
                    Action::RecordHint
                }
            );
            let ranges = History::compile(vec![event(1, Kind::Range, 10_000 - age)]).unwrap();
            assert_eq!(
                ranges.plan(target(), Origin::AppliedBan, 10_000).unwrap(),
                if age < 3600 {
                    Action::Unchanged
                } else {
                    Action::RecordRange
                }
            );
            // A first request records its hint even if a recent direct ban exists.
            assert_eq!(
                ranges.plan(target(), Origin::BanRequest, 10_000).unwrap(),
                Action::RecordHint
            );
        }
        let empty = History::compile(vec![]).unwrap();
        assert_eq!(
            empty.plan(target(), Origin::AppliedBan, 10_000).unwrap(),
            Action::RecordRange
        );
        assert_eq!(
            empty.plan(target(), Origin::BanRequest, 10_000).unwrap(),
            Action::RecordHint
        );
    }

    #[test]
    fn hints_and_duplicate_events_require_exact_scope_and_are_bounded() {
        let history = History::compile(vec![
            event(1, Kind::Hint, 10_000),
            event(2, Kind::Range, 10_000),
        ])
        .unwrap();
        assert_eq!(
            history.plan(target(), Origin::BanRequest, 10_000).unwrap(),
            Action::Unchanged
        );
        for target in [
            Target {
                board: "a",
                ..target()
            },
            Target {
                parent: 0,
                ..target()
            },
            Target {
                prefix: &[2; 32],
                ..target()
            },
            Target {
                browser: "1badc0ffe",
                ..target()
            },
        ] {
            assert_eq!(
                history.plan(target, Origin::BanRequest, 10_000).unwrap(),
                Action::RecordHint
            );
        }
        assert!(
            history
                .plan(
                    Target {
                        browser: "0deadbeef",
                        ..target()
                    },
                    Origin::AppliedBan,
                    10_000
                )
                .is_err()
        );
        assert!(History::compile(vec![event(1, Kind::Hint, 0), event(1, Kind::Range, 0)]).is_err());
        assert!(
            History::compile(
                (0..=MAX_EVENTS)
                    .map(|id| event(id as i64 + 1, Kind::Hint, 0))
                    .collect()
            )
            .is_err()
        );
    }
    #[test]
    fn plans_match_selected_admin_source_with_synthetic_event_history() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../../fixtures/automatic-range-creation-reference.json"
        ))
        .unwrap();
        let cases = corpus["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 96);
        let prefix = |ip: &str| {
            let bytes = ip.parse::<std::net::Ipv4Addr>().unwrap().octets();
            let mut token = [0; 32];
            token[..2].copy_from_slice(&bytes[..2]);
            token
        };
        for (index, case) in cases.iter().enumerate() {
            let events = case["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| HistoryEvent {
                    kind: match entry["kind"].as_str().unwrap() {
                        "hint" => Kind::Hint,
                        "range" => Kind::Range,
                        _ => panic!("Unknown fixture kind"),
                    },
                    event: Event {
                        id: entry["id"].as_i64().unwrap(),
                        board: entry["board"].as_str().unwrap().into(),
                        parent: entry["parent"].as_i64().unwrap(),
                        browser: entry["browser"].as_str().unwrap().into(),
                        prefix: prefix(entry["ip"].as_str().unwrap()),
                        created_at: entry["created_at"].as_u64().unwrap(),
                    },
                })
                .collect();
            let prefix = prefix(case["ip"].as_str().unwrap());
            let target = Target {
                board: "g",
                parent: case["parent"].as_i64().unwrap(),
                prefix: &prefix,
                browser: case["browser"].as_str().unwrap(),
            };
            let origin = if case["origin"] == 1 {
                Origin::AppliedBan
            } else {
                Origin::BanRequest
            };
            let action = History::compile(events)
                .unwrap()
                .plan(target, origin, case["now"].as_u64().unwrap())
                .unwrap();
            let expected = match case["action"].as_str().unwrap() {
                "hint" => Action::RecordHint,
                "range" => Action::RecordRange,
                "unchanged" => Action::Unchanged,
                _ => panic!("Unknown fixture action"),
            };
            assert_eq!(action, expected, "source creation vector {index}");
        }
    }
}
