use std::fmt::{Display, Write};

pub const STORAGE_TARGETS: [&str; 4] = ["database", "quarantine", "public_media", "monitoring"];
pub const SERVICE_TARGETS: [&str; 11] = [
    "public",
    "staff",
    "media_gateway",
    "media_broker",
    "media_reader",
    "queue_observer",
    "resource_observer",
    "database",
    "prometheus",
    "alertmanager",
    "maintenance_observer",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct StorageSample {
    pub capacity_bytes: u64,
    pub available_bytes: u64,
    pub inodes: u64,
    pub available_inodes: u64,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ServiceSample {
    pub memory_bytes: u64,
    pub memory_limit_bytes: u64,
    pub tasks: u64,
    pub tasks_limit: u64,
    pub cpu_usage_usec: u64,
    pub cpu_quota_usec: u64,
    pub cpu_period_usec: u64,
    pub cpu_periods: u64,
    pub cpu_throttled_periods: u64,
    pub memory_oom_kills: u64,
}

/// A completed cache snapshot. None means an unconfigured fixed target slot.
/// Unavailable snapshots emit only success and last-success time, ignoring data.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceSample {
    pub available: bool,
    pub last_success_timestamp_seconds: u64,
    pub storages: [Option<StorageSample>; 4],
    pub services: [Option<ServiceSample>; 11],
}

fn family<S, V: Display, const N: usize>(
    text: &mut String,
    description: (&str, &str, &str),
    label: &str,
    targets: [&str; N],
    samples: &[Option<S>; N],
    value: impl Fn(&S) -> V,
) {
    let (name, kind, help) = description;
    writeln!(text, "# HELP {name} {help}\n# TYPE {name} {kind}").unwrap();
    for (target, sample) in targets.into_iter().zip(samples) {
        if let Some(sample) = sample {
            writeln!(text, "{name}{{{label}=\"{target}\"}} {}", value(sample)).unwrap();
        }
    }
}

pub(crate) fn render(text: &mut String, sample: ResourceSample) {
    writeln!(text, "# HELP board_resource_sample_success Whether a current complete resource snapshot is available.\n# TYPE board_resource_sample_success gauge\nboard_resource_sample_success {}", u64::from(sample.available)).unwrap();
    writeln!(text, "# HELP board_resource_sample_last_success_timestamp_seconds Unix timestamp of the last successful resource sample, or zero before success.\n# TYPE board_resource_sample_last_success_timestamp_seconds gauge\nboard_resource_sample_last_success_timestamp_seconds {}", sample.last_success_timestamp_seconds).unwrap();
    if !sample.available {
        return;
    }
    macro_rules! storage {
        ($field:ident, $help:literal) => {
            family(
                text,
                (
                    concat!("board_storage_", stringify!($field)),
                    "gauge",
                    $help,
                ),
                "storage",
                STORAGE_TARGETS,
                &sample.storages,
                |s| s.$field,
            );
        };
    }
    storage!(capacity_bytes, "Containing filesystem total bytes.");
    storage!(
        available_bytes,
        "Containing filesystem bytes available to unprivileged users."
    );
    storage!(inodes, "Containing filesystem total inodes.");
    storage!(available_inodes, "Containing filesystem available inodes.");
    family(
        text,
        (
            "board_storage_read_only",
            "gauge",
            "Whether the containing filesystem is read-only.",
        ),
        "storage",
        STORAGE_TARGETS,
        &sample.storages,
        |s| u64::from(s.read_only),
    );
    macro_rules! service {
        ($name:literal, $kind:literal, $help:literal, $value:expr) => {
            family(
                text,
                (concat!("board_service_", $name), $kind, $help),
                "service",
                SERVICE_TARGETS,
                &sample.services,
                $value,
            );
        };
    }
    service!(
        "memory_bytes",
        "gauge",
        "Current local cgroup memory bytes.",
        |s| s.memory_bytes
    );
    service!(
        "memory_limit_bytes",
        "gauge",
        "Finite local cgroup memory ceiling in bytes.",
        |s| s.memory_limit_bytes
    );
    service!("tasks", "gauge", "Current local cgroup tasks.", |s| s.tasks);
    service!(
        "tasks_limit",
        "gauge",
        "Finite local cgroup task ceiling.",
        |s| s.tasks_limit
    );
    service!(
        "cpu_usage_seconds_total",
        "counter",
        "Cumulative cgroup CPU seconds; may reset on recreation.",
        |s| s.cpu_usage_usec as f64 / 1_000_000.0
    );
    service!(
        "cpu_quota_cores",
        "gauge",
        "Finite local cgroup CPU quota divided by its period.",
        |s| s.cpu_quota_usec as f64 / s.cpu_period_usec as f64
    );
    service!(
        "cpu_periods_total",
        "counter",
        "Cumulative cgroup CPU periods; may reset on recreation.",
        |s| s.cpu_periods
    );
    service!(
        "cpu_throttled_periods_total",
        "counter",
        "Cumulative throttled cgroup CPU periods; may reset on recreation.",
        |s| s.cpu_throttled_periods
    );
    service!(
        "memory_oom_kills_total",
        "counter",
        "Cumulative cgroup OOM kills; may reset on recreation.",
        |s| s.memory_oom_kills
    );
}
