use super::*;

const GOOD: [&[u8]; 7] = [
    b"2048\n", b"4096\n", b"low 0\nhigh 1\nmax 2\noom 3\noom_kill 4\nfuture_event 5\n",
    b"7\n", b"32\n", b"usage_usec 1234567\nuser_usec 765432\nsystem_usec 469135\nnr_periods 50\nnr_throttled 10\nthrottled_usec 9000\nfuture_stat 8\n",
    b"25000 100000\n",
];

#[test]
fn parses_actual_cgroup_shapes_with_finite_local_limits_and_unknown_keys() {
    let sample = parse_service(GOOD).unwrap();
    assert_eq!(sample.memory_bytes, 2048);
    assert_eq!(sample.memory_limit_bytes, 4096);
    assert_eq!(sample.memory_oom_kills, 4);
    assert_eq!(sample.tasks, 7);
    assert_eq!(sample.tasks_limit, 32);
    assert_eq!(sample.cpu_usage_usec, 1234567);
    assert_eq!(sample.cpu_quota_usec, 25000);
    assert_eq!(sample.cpu_period_usec, 100000);
    assert_eq!(sample.cpu_periods, 50);
    assert_eq!(sample.cpu_throttled_periods, 10);
    let mut above = GOOD;
    above[0] = b"5000";
    above[3] = b"40";
    assert!(
        parse_service(above).is_ok(),
        "usage can exceed a lowered local ceiling"
    );
}

#[test]
fn rejects_unlimited_zero_invalid_overflowing_or_missing_required_values() {
    for (slot, malformed) in [
        (0, &b"-1"[..]),
        (0, b"+1"),
        (0, b"1 2"),
        (0, b"18446744073709551616"),
        (1, b"max"),
        (1, b"0"),
        (3, b""),
        (4, b"max"),
        (4, b"0"),
        (6, b"max 100000"),
        (6, b"1 0"),
        (6, b"0 100000"),
        (6, b"1"),
        (6, b"1 2 3"),
        (2, b"oom 4\n"),
        (2, b"oom_kill 1\noom_kill 2\n"),
        (2, b"oom_kill -1\n"),
        (2, b"oom_kill 1\nfuture 0\nfuture 1\n"),
        (2, b"oom_kill 1\nfuture wrong\n"),
        (5, b"usage_usec 1\nnr_periods 1\n"),
        (
            5,
            b"usage_usec 1\nnr_periods 1\nnr_throttled 0\nusage_usec 2\n",
        ),
        (
            5,
            b"usage_usec 18446744073709551616\nnr_periods 1\nnr_throttled 0\n",
        ),
    ] {
        let mut files = GOOD;
        files[slot] = malformed;
        assert!(
            parse_service(files).is_err(),
            "accepted malformed slot {slot}"
        );
    }
    let oversized = vec![b' '; 4097];
    let mut files = GOOD;
    files[2] = &oversized;
    assert!(parse_service(files).is_err());
}

#[test]
fn uses_available_blocks_and_checked_filesystem_arithmetic() {
    let sample = storage_sample(100, 25, 4096, 1000, 400, true).unwrap();
    assert_eq!(sample.capacity_bytes, 409600);
    assert_eq!(sample.available_bytes, 102400);
    assert_eq!(sample.inodes, 1000);
    assert_eq!(sample.available_inodes, 400);
    assert!(sample.read_only);
    for args in [
        (0, 0, 4096, 1, 1),
        (1, 0, 0, 1, 1),
        (1, 2, 4096, 1, 1),
        (1, 0, 4096, 0, 0),
        (1, 0, 4096, 1, 2),
        (u64::MAX, 0, 4096, 1, 1),
    ] {
        assert!(storage_sample(args.0, args.1, args.2, args.3, args.4, false).is_err());
    }
}

proptest::proptest! {
    #[test]
    fn arbitrary_source_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..5000)) {
        let mut files = GOOD; files[5] = &bytes;
        let _ = parse_service(files);
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn unsupported_os_never_returns_fabricated_measurements() {
    let targets = Targets {
        storages: Default::default(),
        services: Default::default(),
    };
    assert!(collect(&targets).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn native_storage_uses_directory_metadata_and_rejects_disappeared_sources() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let storage = root.join("storage");
    std::fs::create_dir(&storage).unwrap();
    std::fs::write(storage.join("protected-payload"), b"not a measurement").unwrap();
    std::fs::set_permissions(
        storage.join("protected-payload"),
        std::fs::Permissions::from_mode(0o0),
    )
    .unwrap();
    std::fs::set_permissions(&storage, std::fs::Permissions::from_mode(0o111)).unwrap();
    let observed = linux::storage(&storage);
    std::fs::set_permissions(&storage, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sample = observed.unwrap();
    assert!(sample.capacity_bytes > 0);
    assert!(sample.available_bytes <= sample.capacity_bytes);
    assert!(sample.inodes > 0);
    assert!(sample.available_inodes <= sample.inodes);
    std::fs::remove_file(storage.join("protected-payload")).unwrap();
    std::fs::remove_dir(&storage).unwrap();
    assert!(linux::storage(&storage).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn native_cgroup_collection_rejects_regular_files_on_other_filesystems() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    for (name, content) in [
        "memory.current",
        "memory.max",
        "memory.events",
        "pids.current",
        "pids.max",
        "cpu.stat",
        "cpu.max",
    ]
    .iter()
    .zip(GOOD)
    {
        std::fs::write(root.join(name), content).unwrap();
    }
    let targets = Targets {
        storages: [Some(root.clone()), None, None, None],
        services: std::array::from_fn(|i| (i == 0).then(|| root.clone())),
    };
    assert!(
        collect(&targets).is_err(),
        "fabricated numeric files must never count as native cgroup evidence"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn native_component_open_rejects_symlinks_and_nonblocking_file_read_rejects_fifo() {
    use rustix::fs::{Mode, mkfifoat};
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("real")).unwrap();
    symlink(root.join("real"), root.join("alias")).unwrap();
    assert!(crate::config::open_directory(&root.join("alias")).is_err());
    let fd = crate::config::open_directory(&root).unwrap();
    mkfifoat(&fd, "memory.current", Mode::RUSR | Mode::WUSR).unwrap();
    assert!(linux::read_stat(&fd, "memory.current").is_err());
    std::fs::remove_file(root.join("memory.current")).unwrap();
    symlink(root.join("real"), root.join("memory.current")).unwrap();
    assert!(linux::read_stat(&fd, "memory.current").is_err());
}
