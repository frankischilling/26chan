use super::*;

fn sample() -> ResourceSample {
    ResourceSample {
        available: true,
        last_success_timestamp_seconds: 123,
        storages: std::array::from_fn(|i| {
            Some(StorageSample {
                capacity_bytes: 1000 + i as u64,
                available_bytes: 400 + i as u64,
                inodes: 100 + i as u64,
                available_inodes: 40 + i as u64,
                read_only: i == 3,
            })
        }),
        services: std::array::from_fn(|i| {
            Some(ServiceSample {
                memory_bytes: 100 + i as u64,
                memory_limit_bytes: 200 + i as u64,
                tasks: 3 + i as u64,
                tasks_limit: 20 + i as u64,
                cpu_usage_usec: 1_500_000,
                cpu_quota_usec: 50_000,
                cpu_period_usec: 100_000,
                cpu_periods: 1234 + i as u64,
                cpu_throttled_periods: 12 + i as u64,
                memory_oom_kills: i as u64,
            })
        }),
    }
}

#[test]
fn resource_families_have_fixed_slots_types_and_native_unit_conversions() {
    assert_eq!(
        STORAGE_TARGETS,
        ["database", "quarantine", "public_media", "monitoring"]
    );
    assert_eq!(
        SERVICE_TARGETS,
        [
            "public",
            "staff",
            "media_gateway",
            "media_broker",
            "media_reader",
            "queue_observer",
            "resource_observer",
            "database",
            "prometheus",
            "alertmanager"
        ]
    );
    let mut metrics = Metrics::new();
    metrics.register_resources(sample).unwrap();
    let text = metrics.render();
    assert!(text.contains("board_resource_sample_success 1\n"));
    assert!(text.contains("board_resource_sample_last_success_timestamp_seconds 123\n"));
    for (i, target) in STORAGE_TARGETS.iter().enumerate() {
        for (name, value) in [
            ("capacity_bytes", 1000 + i),
            ("available_bytes", 400 + i),
            ("inodes", 100 + i),
            ("available_inodes", 40 + i),
            ("read_only", usize::from(i == 3)),
        ] {
            assert!(text.contains(&format!("# TYPE board_storage_{name} gauge\n")));
            assert!(text.contains(&format!(
                "board_storage_{name}{{storage=\"{target}\"}} {value}\n"
            )));
        }
    }
    for (i, target) in SERVICE_TARGETS.iter().enumerate() {
        for (name, value, kind) in [
            ("memory_bytes", (100 + i).to_string(), "gauge"),
            ("memory_limit_bytes", (200 + i).to_string(), "gauge"),
            ("tasks", (3 + i).to_string(), "gauge"),
            ("tasks_limit", (20 + i).to_string(), "gauge"),
            ("cpu_usage_seconds_total", "1.5".into(), "counter"),
            ("cpu_quota_cores", "0.5".into(), "gauge"),
            ("cpu_periods_total", (1234 + i).to_string(), "counter"),
            (
                "cpu_throttled_periods_total",
                (12 + i).to_string(),
                "counter",
            ),
            ("memory_oom_kills_total", i.to_string(), "counter"),
        ] {
            assert!(text.contains(&format!("# TYPE board_service_{name} {kind}\n")));
            assert!(text.contains(&format!(
                "board_service_{name}{{service=\"{target}\"}} {value}\n"
            )));
        }
    }
    let values: Vec<_> = text
        .lines()
        .filter(|line| {
            !line.starts_with('#')
                && (line.starts_with("board_resource_")
                    || line.starts_with("board_storage_")
                    || line.starts_with("board_service_"))
        })
        .collect();
    assert_eq!(values.len(), 2 + 4 * 5 + 10 * 9);
    assert!(text.len() < 32_768);
}

#[test]
fn resource_callback_is_called_once_per_scrape_and_unavailable_data_is_omitted() {
    let calls = Arc::new(AtomicU64::new(0));
    let seen = calls.clone();
    let mut metrics = Metrics::new();
    metrics
        .register_resources(move || {
            let mut value = sample();
            value.available = seen.fetch_add(1, Ordering::Relaxed) == 0;
            value
        })
        .unwrap();
    assert!(metrics.render().contains("board_storage_capacity_bytes{"));
    let failed = metrics.render();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(failed.contains("board_resource_sample_success 0\n"));
    assert!(failed.contains("board_resource_sample_last_success_timestamp_seconds 123\n"));
    assert!(!failed.contains("board_storage_"));
    assert!(!failed.contains("board_service_"));
}

#[test]
fn absent_resources_emit_no_target_values_and_registration_is_unique_before_sharing() {
    let mut metrics = Metrics::new();
    assert!(!metrics.render().contains("board_resource_"));
    metrics
        .register_resources(|| ResourceSample {
            available: true,
            ..Default::default()
        })
        .unwrap();
    assert!(metrics.register_resources(ResourceSample::default).is_err());
    let text = metrics.render();
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("board_storage_") || line.starts_with("board_service_"))
    );
    let mut shared = Metrics::new();
    let _clone = shared.clone();
    assert!(shared.register_resources(ResourceSample::default).is_err());
}

#[test]
fn sparse_targets_do_not_leak_default_slots_and_maximum_values_remain_bounded() {
    let mut value = ResourceSample {
        available: true,
        ..Default::default()
    };
    value.storages[2] = Some(StorageSample {
        capacity_bytes: u64::MAX,
        available_bytes: u64::MAX,
        inodes: u64::MAX,
        available_inodes: u64::MAX,
        read_only: false,
    });
    value.services[9] = Some(ServiceSample {
        cpu_usage_usec: u64::MAX,
        cpu_quota_usec: u64::MAX,
        cpu_period_usec: 1,
        ..Default::default()
    });
    let mut metrics = Metrics::new();
    metrics.register_resources(move || value).unwrap();
    let text = metrics.render();
    assert!(
        text.contains(
            "board_storage_capacity_bytes{storage=\"public_media\"} 18446744073709551615\n"
        )
    );
    assert!(!text.contains("storage=\"database\""));
    assert!(!text.contains("service=\"public\""));
    assert!(text.len() < 32_768);
}
