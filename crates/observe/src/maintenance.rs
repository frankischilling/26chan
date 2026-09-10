use std::fmt::Write;

pub const MAINTENANCE_TARGETS: [&str; 4] = ["application", "host", "media_guest", "monitoring"];

/// A completed memory-only observation of one configured journal.
/// Zero last_success_ms means no successful command has been recorded.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaintenanceTargetSample {
    pub available: bool,
    pub last_sample_timestamp_seconds: u64,
    pub running: bool,
    pub failure_pending: bool,
    pub started_ms: u64,
    pub last_success_ms: u64,
    pub max_age_seconds: u64,
    pub run_timeout_seconds: u64,
}

/// None means unconfigured. Configured unavailable targets retain only their
/// sample-success indicator and last successful observation time in exposition.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaintenanceSample {
    pub targets: [Option<MaintenanceTargetSample>; 4],
}

pub(crate) fn render(text: &mut String, sample: MaintenanceSample) {
    if sample.targets.iter().all(Option::is_none) {
        return;
    }
    let any_available = sample
        .targets
        .iter()
        .flatten()
        .any(|target| target.available);
    for (name, help) in [
        (
            "sample_success",
            "Whether this configured journal has a current valid sample.",
        ),
        (
            "sample_last_success_timestamp_seconds",
            "Unix timestamp of the last successful journal observation, or zero before success.",
        ),
        (
            "run_in_progress",
            "Whether the recorded maintenance attempt is running.",
        ),
        (
            "failure_pending",
            "Whether a recorded maintenance failure remains uncleared by success.",
        ),
        (
            "run_started_timestamp_seconds",
            "Unix timestamp when the recorded maintenance attempt began.",
        ),
        (
            "last_success_timestamp_seconds",
            "Unix timestamp of the last successful maintenance command, or zero before success.",
        ),
        (
            "max_age_seconds",
            "Configured maximum age of a successful maintenance command.",
        ),
        (
            "run_timeout_seconds",
            "Configured maximum age of a running maintenance attempt.",
        ),
    ] {
        if !any_available
            && !matches!(
                name,
                "sample_success" | "sample_last_success_timestamp_seconds"
            )
        {
            continue;
        }
        writeln!(
            text,
            "# HELP board_maintenance_{name} {help}\n# TYPE board_maintenance_{name} gauge"
        )
        .unwrap();
        for (target, slot) in MAINTENANCE_TARGETS.into_iter().zip(&sample.targets) {
            let Some(value) = slot else { continue };
            if !value.available
                && !matches!(
                    name,
                    "sample_success" | "sample_last_success_timestamp_seconds"
                )
            {
                continue;
            }
            let number = match name {
                "sample_success" => u64::from(value.available),
                "sample_last_success_timestamp_seconds" => value.last_sample_timestamp_seconds,
                "run_in_progress" => u64::from(value.running),
                "failure_pending" => u64::from(value.failure_pending),
                "run_started_timestamp_seconds" => value.started_ms,
                "last_success_timestamp_seconds" => value.last_success_ms,
                "max_age_seconds" => value.max_age_seconds,
                "run_timeout_seconds" => value.run_timeout_seconds,
                _ => unreachable!(),
            };
            if matches!(
                name,
                "run_started_timestamp_seconds" | "last_success_timestamp_seconds"
            ) {
                // Format exact milliseconds without rounding through f64.
                writeln!(
                    text,
                    "board_maintenance_{name}{{maintenance=\"{target}\"}} {}.{:03}",
                    number / 1000,
                    number % 1000
                )
                .unwrap();
            } else {
                writeln!(
                    text,
                    "board_maintenance_{name}{{maintenance=\"{target}\"}} {number}"
                )
                .unwrap();
            }
        }
    }
}
