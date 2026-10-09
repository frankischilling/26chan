//! Fixed-schema fixture-owned checkpoints. No browser/kernel lifetime inference.
use serde::Serialize;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};

const LIMIT: u32 = u32::MAX;
const ERROR_CHECKPOINTS: u32 = 16;

#[derive(Clone, Default, Serialize)]
struct Counts {
    accepted: u32,
    active: u32,
    peak_active: u32,
    completed: u32,
    cancelled: u32,
    http_parse: u32,
    http_incomplete: u32,
    http_timeout: u32,
    http_other: u32,
    accept_errors: u32,
}

#[derive(Default)]
struct State {
    counts: Counts,
    overflow: bool,
    output_failed: bool,
    error_checkpoints: u32,
    sequence: u32,
}

#[derive(Clone, Default)]
pub struct Lifecycle(Option<Arc<Mutex<State>>>);

#[derive(Serialize)]
struct Snapshot<'a> {
    schema_version: u32,
    scope: &'static str,
    boundary: &'static str,
    sequence: u32,
    counters: &'a Counts,
    overflow: bool,
    output_failed: bool,
    complete: bool,
    browser_lifetime: &'static str,
}

impl State {
    fn ended(&self) -> u64 {
        let c = &self.counts;
        [
            c.completed,
            c.cancelled,
            c.http_parse,
            c.http_incomplete,
            c.http_timeout,
            c.http_other,
        ]
        .into_iter()
        .map(u64::from)
        .sum()
    }

    fn emit(&mut self, boundary: &'static str) {
        self.sequence += 1; // At most 82 snapshots under the fixed checkpoint policy.
        let record = Snapshot {
            schema_version: 1,
            scope: "fixture-listeners",
            boundary,
            sequence: self.sequence,
            counters: &self.counts,
            overflow: self.overflow,
            output_failed: self.output_failed,
            complete: boundary == "shutdown"
                && !self.overflow
                && !self.output_failed
                && self.counts.active == 0
                && self.ended() == u64::from(self.counts.accepted),
            browser_lifetime: "unavailable",
        };
        // Write only a fixed marker and serialized scalar schema. Failed output
        // cannot panic or change service behavior; later snapshots mark it lost.
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        if write!(output, "[owned-fixture-lifecycle] ").is_err()
            || serde_json::to_writer(&mut output, &record).is_err()
            || writeln!(output).is_err()
            || output.flush().is_err()
        {
            self.output_failed = true;
        }
    }

    fn error_checkpoint(&mut self) {
        if self.error_checkpoints < ERROR_CHECKPOINTS {
            self.error_checkpoints += 1;
            self.emit("error");
        }
    }

    fn increment(value: &mut u32) -> bool {
        if *value == LIMIT {
            false
        } else {
            *value += 1;
            true
        }
    }
}

impl Lifecycle {
    pub fn from_env() -> Self {
        if cfg!(windows)
            && std::env::var_os("WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            let this = Self(Some(Arc::new(Mutex::new(State::default()))));
            this.with(|state| state.emit("startup"));
            this
        } else {
            Self::default()
        }
    }

    fn with(&self, action: impl FnOnce(&mut State)) {
        if let Some(state) = &self.0 {
            // No user code runs under this lock. Poison still marks evidence
            // incomplete rather than changing the original fixture outcome.
            match state.lock() {
                Ok(mut state) => action(&mut state),
                Err(error) => {
                    let mut state = error.into_inner();
                    state.overflow = true;
                    action(&mut state);
                }
            }
        }
    }

    pub fn accepted(&self) -> Connection {
        self.with(|state| {
            if state.overflow {
                return;
            }
            let c = &mut state.counts;
            if !State::increment(&mut c.accepted) || !State::increment(&mut c.active) {
                state.overflow = true;
                state.error_checkpoint();
                return;
            }
            c.peak_active = c.peak_active.max(c.active);
            if c.accepted.is_power_of_two() {
                state.emit("accepted");
            }
        });
        Connection {
            lifecycle: self.clone(),
            ended: false,
        }
    }

    pub fn accept_error(&self) {
        self.with(|state| {
            if !state.overflow {
                if !State::increment(&mut state.counts.accept_errors) {
                    state.overflow = true;
                }
                state.error_checkpoint();
            }
        });
    }

    pub fn shutdown(&self) {
        self.with(|state| state.emit("shutdown"));
    }
}

pub struct Connection {
    lifecycle: Lifecycle,
    ended: bool,
}

impl Connection {
    pub fn finish(mut self, result: Result<(), hyper::Error>) {
        let class = match result {
            Ok(()) => 0,
            Err(error) if error.is_parse() => 2,
            Err(error) if error.is_incomplete_message() => 3,
            Err(error) if error.is_timeout() => 4,
            Err(_) => 5,
        };
        self.end(class);
    }

    fn end(&mut self, class: u8) {
        if self.ended {
            return;
        }
        self.ended = true;
        self.lifecycle.with(|state| {
            if state.overflow {
                return;
            }
            let c = &mut state.counts;
            let counter = match class {
                0 => &mut c.completed,
                1 => &mut c.cancelled,
                2 => &mut c.http_parse,
                3 => &mut c.http_incomplete,
                4 => &mut c.http_timeout,
                _ => &mut c.http_other,
            };
            if c.active == 0 || !State::increment(counter) {
                state.overflow = true;
                state.error_checkpoint();
                return;
            }
            c.active -= 1;
            if class >= 2 {
                state.error_checkpoint();
            } else if state.ended().is_power_of_two() {
                state.emit("ended");
            }
        });
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.end(1);
    }
}

#[cfg(test)]
impl Lifecycle {
    pub(super) fn for_test() -> Self {
        Self(Some(Arc::new(Mutex::new(State::default()))))
    }

    pub(super) fn test_counts(&self) -> (u32, u32, u32) {
        let state = self.0.as_ref().unwrap().lock().unwrap();
        (
            state.counts.accepted,
            state.counts.active,
            state.counts.cancelled,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_never_wraps() {
        let mut value = LIMIT - 1;
        assert!(State::increment(&mut value));
        assert_eq!(value, LIMIT);
        assert!(!State::increment(&mut value));
        assert_eq!(value, LIMIT);
    }

    #[test]
    fn connection_guard_reconciles_completion_and_cancellation() {
        let state = Arc::new(Mutex::new(State::default()));
        let lifecycle = Lifecycle(Some(state.clone()));
        let first = lifecycle.accepted();
        let second = lifecycle.accepted();
        first.finish(Ok(()));
        drop(second);
        let state = state.lock().unwrap();
        assert_eq!(state.counts.accepted, 2);
        assert_eq!(state.counts.active, 0);
        assert_eq!(state.counts.peak_active, 2);
        assert_eq!(state.counts.completed, 1);
        assert_eq!(state.counts.cancelled, 1);
        assert_eq!(state.ended(), 2);
        assert!(!state.overflow);
    }

    #[test]
    fn error_classes_are_disjoint_and_error_checkpoints_are_bounded() {
        let state = Arc::new(Mutex::new(State::default()));
        let lifecycle = Lifecycle(Some(state.clone()));
        for class in 2..=5 {
            let mut connection = lifecycle.accepted();
            connection.end(class);
        }
        for _ in 0..20 {
            lifecycle.accept_error();
        }
        let state = state.lock().unwrap();
        let counts = &state.counts;
        assert_eq!(
            [
                counts.http_parse,
                counts.http_incomplete,
                counts.http_timeout,
                counts.http_other
            ],
            [1; 4]
        );
        assert_eq!(counts.completed, 0);
        assert_eq!(counts.cancelled, 0);
        assert_eq!(counts.active, 0);
        assert_eq!(counts.accept_errors, 20);
        assert_eq!(state.error_checkpoints, ERROR_CHECKPOINTS);
        assert_eq!(state.sequence, 3 + ERROR_CHECKPOINTS);
    }

    #[test]
    fn overflow_is_sticky_and_freezes_counts_without_stopping_service() {
        let state = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().counts.accepted = LIMIT;
        let lifecycle = Lifecycle(Some(state.clone()));
        drop(lifecycle.accepted());
        lifecycle.accept_error();
        let state = state.lock().unwrap();
        assert!(state.overflow);
        assert_eq!(state.counts.accepted, LIMIT);
        assert_eq!(state.counts.cancelled, 0);
        assert_eq!(state.counts.accept_errors, 0);
    }
}
