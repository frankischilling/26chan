use super::*;

fn maintenance_sample() -> MaintenanceSample {
    MaintenanceSample {
        targets: std::array::from_fn(|index| {
            Some(MaintenanceTargetSample {
                available: true,
                last_sample_timestamp_seconds: 123 + index as u64,
                running: index == 1,
                failure_pending: index == 1 || index == 2,
                started_ms: 1_234_567 + index as u64,
                last_success_ms: if index == 3 {
                    0
                } else {
                    1_200_001 + index as u64
                },
                max_age_seconds: 604_800,
                run_timeout_seconds: 1800,
            })
        }),
    }
}

#[test]
fn maintenance_uses_only_closed_targets_and_exact_fractional_timestamps() {
    assert_eq!(
        MAINTENANCE_TARGETS,
        ["application", "host", "media_guest", "monitoring"]
    );
    let mut metrics = Metrics::new();
    metrics.register_maintenance(maintenance_sample).unwrap();
    let text = metrics.render();
    for (index, target) in MAINTENANCE_TARGETS.iter().enumerate() {
        for (name, expected) in [
            ("sample_success", "1".to_owned()),
            (
                "sample_last_success_timestamp_seconds",
                (123 + index).to_string(),
            ),
            ("run_in_progress", u64::from(index == 1).to_string()),
            (
                "failure_pending",
                u64::from(index == 1 || index == 2).to_string(),
            ),
            (
                "run_started_timestamp_seconds",
                format!("1234.{}", 567 + index),
            ),
            (
                "last_success_timestamp_seconds",
                if index == 3 {
                    "0.000".to_owned()
                } else {
                    format!("1200.{:03}", 1 + index)
                },
            ),
            ("max_age_seconds", "604800".to_owned()),
            ("run_timeout_seconds", "1800".to_owned()),
        ] {
            assert!(text.contains(&format!("# TYPE board_maintenance_{name} gauge\n")));
            assert!(text.contains(&format!(
                "board_maintenance_{name}{{maintenance=\"{target}\"}} {expected}\n"
            )));
        }
    }
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("board_maintenance_"))
            .count(),
        32
    );
    assert!(text.len() < 32_768);
}

#[test]
fn unavailable_target_drops_only_its_data_and_keeps_sample_history() {
    let mut sample = maintenance_sample();
    sample.targets[0].as_mut().unwrap().available = false;
    sample.targets[2] = None;
    let mut metrics = Metrics::new();
    metrics.register_maintenance(move || sample).unwrap();
    let text = metrics.render();
    let application: Vec<_> = text
        .lines()
        .filter(|line| line.contains("maintenance=\"application\""))
        .collect();
    assert_eq!(
        application,
        [
            "board_maintenance_sample_success{maintenance=\"application\"} 0",
            "board_maintenance_sample_last_success_timestamp_seconds{maintenance=\"application\"} 123",
        ]
    );
    assert!(text.contains("board_maintenance_failure_pending{maintenance=\"host\"} 1\n"));
    assert!(text.contains("board_maintenance_run_in_progress{maintenance=\"host\"} 1\n"));
    assert!(!text.contains("maintenance=\"media_guest\""));
}

#[test]
fn maintenance_registration_is_unique_before_sharing_and_callback_runs_once() {
    let calls = Arc::new(AtomicU64::new(0));
    let observed = calls.clone();
    let mut metrics = Metrics::new();
    assert!(!metrics.render().contains("board_maintenance_"));
    metrics
        .register_maintenance(move || {
            observed.fetch_add(1, Ordering::Relaxed);
            MaintenanceSample::default()
        })
        .unwrap();
    assert!(
        metrics
            .register_maintenance(MaintenanceSample::default)
            .is_err()
    );
    let text = metrics.render();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!text.contains("board_maintenance_"));
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("board_maintenance_"))
    );
    let mut shared = Metrics::new();
    let _other = shared.clone();
    assert!(
        shared
            .register_maintenance(MaintenanceSample::default)
            .is_err()
    );
}

#[test]
fn large_millisecond_values_render_without_floating_point_rounding() {
    let mut sample = MaintenanceSample::default();
    sample.targets[0] = Some(MaintenanceTargetSample {
        available: true,
        started_ms: 9_007_199_254_740_991,
        last_success_ms: 1001,
        ..Default::default()
    });
    let mut metrics = Metrics::new();
    metrics.register_maintenance(move || sample).unwrap();
    let text = metrics.render();
    assert!(text.contains("board_maintenance_run_started_timestamp_seconds{maintenance=\"application\"} 9007199254740.991\n"));
    assert!(text.contains(
        "board_maintenance_last_success_timestamp_seconds{maintenance=\"application\"} 1.001\n"
    ));
}

#[test]
fn entirely_unavailable_maintenance_exports_only_observation_families() {
    let mut metrics = Metrics::new();
    metrics
        .register_maintenance(|| MaintenanceSample {
            targets: [Some(MaintenanceTargetSample::default()); 4],
        })
        .unwrap();
    let text = metrics.render();
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("# TYPE board_maintenance_"))
            .count(),
        2
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("board_maintenance_"))
            .count(),
        8
    );
    assert!(!text.contains("board_maintenance_run_"));
    assert!(!text.contains("board_maintenance_failure_pending"));
    assert!(!text.contains("board_maintenance_last_success_timestamp_seconds"));
}
