#[cfg(any(target_os = "linux", test))]
use crate::config::Target;
use crate::config::Targets;
use board_observe::{MaintenanceSample, MaintenanceTargetSample};
#[cfg(any(target_os = "linux", test))]
use serde::Deserialize;

/// No per-target error contains source paths or journal content.
pub fn collect(targets: &Targets) -> MaintenanceSample {
    #[cfg(target_os = "linux")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok());
        match now {
            Some(now) if now > 0 && now < (1_u64 << 53) => collect_at(targets, now),
            _ => unavailable(targets),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        unavailable(targets)
    }
}

pub(crate) fn unavailable(targets: &Targets) -> MaintenanceSample {
    MaintenanceSample {
        targets: std::array::from_fn(|index| {
            targets.targets[index]
                .as_ref()
                .map(|target| MaintenanceTargetSample {
                    max_age_seconds: target.max_age_seconds,
                    run_timeout_seconds: target.run_timeout_seconds,
                    ..Default::default()
                })
        }),
    }
}

#[cfg(target_os = "linux")]
fn collect_at(targets: &Targets, now_ms: u64) -> MaintenanceSample {
    let mut sample = unavailable(targets);
    for (index, target) in targets.targets.iter().enumerate() {
        if let Some(target) = target {
            let observed = crate::config::read_source(&target.path, targets.production, 4096)
                .ok()
                .and_then(|bytes| {
                    parse(
                        &bytes,
                        board_observe::MAINTENANCE_TARGETS[index],
                        target,
                        now_ms,
                    )
                    .ok()
                });
            if let Some(observed) = observed {
                sample.targets[index] = Some(observed);
            }
        }
    }
    sample
}

#[cfg(any(target_os = "linux", test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u64,
    target: String,
    started_ms: u64,
    // Value makes these fields mandatory while allowing explicit JSON null.
    // Option fields alone would silently accept a missing required key.
    finished_ms: serde_json::Value,
    outcome: Outcome,
    last_success_ms: serde_json::Value,
    failure_pending: bool,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Running,
    Success,
    Failure,
}

#[cfg(any(target_os = "linux", test))]
fn parse(
    bytes: &[u8],
    name: &str,
    target: &Target,
    now_ms: u64,
) -> Result<MaintenanceTargetSample, ()> {
    if bytes.len() > 4096 || now_ms == 0 || now_ms >= (1_u64 << 53) {
        return Err(());
    }
    let journal: Journal = serde_json::from_slice(bytes).map_err(|_| ())?;
    fn timestamp(value: u64, now_ms: u64) -> Result<u64, ()> {
        if value == 0 || value >= (1_u64 << 53) || value > now_ms {
            return Err(());
        }
        Ok(value)
    }
    fn optional(value: &serde_json::Value, now_ms: u64) -> Result<Option<u64>, ()> {
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(timestamp(value.as_u64().ok_or(())?, now_ms)?))
        }
    }
    if journal.schema != 1 || journal.target != name {
        return Err(());
    }
    let started = timestamp(journal.started_ms, now_ms)?;
    let finished = optional(&journal.finished_ms, now_ms)?;
    let success = optional(&journal.last_success_ms, now_ms)?;
    if finished.is_some_and(|finished| finished < started) {
        return Err(());
    }
    let running = matches!(journal.outcome, Outcome::Running);
    match journal.outcome {
        Outcome::Running
            if finished.is_none() && success.is_none_or(|success| success <= started) => {}
        Outcome::Success
            if finished.is_some() && success == finished && !journal.failure_pending => {}
        Outcome::Failure
            if finished.is_some()
                && journal.failure_pending
                && success.is_none_or(|success| success <= started) => {}
        _ => return Err(()),
    }
    Ok(MaintenanceTargetSample {
        available: true,
        last_sample_timestamp_seconds: 0,
        running,
        failure_pending: journal.failure_pending,
        started_ms: started,
        last_success_ms: success.unwrap_or(0),
        max_age_seconds: target.max_age_seconds,
        run_timeout_seconds: target.run_timeout_seconds,
    })
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
