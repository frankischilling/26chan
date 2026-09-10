use super::sampler::{SampleState, sample_loop_with_period};
use board_observe::{ResourceSample, ServiceSample, StorageSample};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

fn sample() -> ResourceSample {
    let mut sample = ResourceSample {
        available: true,
        ..ResourceSample::default()
    };
    sample.storages[0] = Some(StorageSample {
        capacity_bytes: 1024,
        available_bytes: 900,
        inodes: 100,
        available_inodes: 90,
        read_only: false,
    });
    sample.services[0] = Some(ServiceSample {
        memory_bytes: 10,
        memory_limit_bytes: 100,
        tasks: 1,
        tasks_limit: 16,
        cpu_period_usec: 100_000,
        cpu_quota_usec: 20_000,
        ..ServiceSample::default()
    });
    sample
}

#[test]
fn cache_uses_monotonic_age_and_invalidates_failed_data_immediately() {
    let state = SampleState::default();
    let now = Instant::now();
    assert!(!state.snapshot_at(now).available);
    state.success_at(sample(), now, 123);
    assert!(state.snapshot_at(now + Duration::from_secs(30)).available);
    assert!(
        !state
            .snapshot_at(now + Duration::from_millis(30_001))
            .available
    );
    state.failure();
    let failed = state.snapshot_at(now);
    assert!(!failed.available);
    assert_eq!(failed.last_success_timestamp_seconds, 123);
    state.success_at(sample(), now + Duration::from_secs(40), 120);
    assert!(state.snapshot_at(now + Duration::from_secs(41)).available);
    assert_eq!(state.snapshot().last_success_timestamp_seconds, 120);
}

#[tokio::test]
async fn failed_collection_recovers_without_exposing_old_success() {
    let state = SampleState::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let (allow_failure, failure_gate) = std::sync::mpsc::channel();
    let failure_gate = Mutex::new(failure_gate);
    let (allow_recovery, recovery_gate) = std::sync::mpsc::channel();
    let recovery_gate = Mutex::new(recovery_gate);
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            let call = attempts.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                failure_gate
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Err(())
            } else {
                if call == 2 {
                    recovery_gate
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
                Ok(sample())
            }
        },
        stopped,
        Duration::from_millis(100),
        Duration::from_secs(2),
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !state.snapshot().available {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        allow_failure.send(()).unwrap();
        while calls.load(Ordering::SeqCst) < 2 || state.snapshot().available {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_ne!(state.snapshot().last_success_timestamp_seconds, 0);
        allow_recovery.send(()).unwrap();
        while !state.snapshot().available {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    stop.send(true).unwrap();
    task.await.unwrap();
    assert!(calls.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn completed_operation_polled_after_deadline_cannot_publish_success() {
    let state = SampleState::default();
    let (started, mut start) = tokio::sync::mpsc::channel(1);
    let (finished, finish) = std::sync::mpsc::channel();
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            started.blocking_send(()).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            finished.send(()).unwrap();
            Ok::<_, ()>(sample())
        },
        stopped,
        Duration::from_secs(3600),
        Duration::from_millis(30),
    ));
    tokio::time::timeout(Duration::from_secs(2), start.recv())
        .await
        .unwrap()
        .unwrap();
    // Deliberately hold the sole async worker while the blocking operation
    // finishes after its deadline. Its JoinHandle is ready at the next poll.
    finish.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    tokio::task::yield_now().await;
    let published = state.snapshot().available;
    stop.send(true).unwrap();
    task.await.unwrap();
    assert!(
        !published,
        "A late ready result must not get a fresh cache lifetime"
    );
}

#[tokio::test]
async fn timeout_retains_blocking_admission_and_shutdown_does_not_wait_for_it() {
    let state = SampleState::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let (release, blocked) = std::sync::mpsc::channel();
    let blocked = Arc::new(Mutex::new(blocked));
    let (finished, mut done) = tokio::sync::mpsc::channel(1);
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            // A bounded test wait; the real collector may be stuck in a kernel
            // call that a Tokio timeout cannot cancel.
            let _ = blocked.lock().unwrap().recv_timeout(Duration::from_secs(5));
            let _ = finished.blocking_send(());
            Ok::<_, ()>(sample())
        },
        stopped,
        Duration::from_millis(10),
        Duration::from_millis(30),
    ));
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!state.snapshot().available);
    stop.send(true).unwrap();
    let stopped_promptly = tokio::time::timeout(Duration::from_millis(200), task).await;
    release.send(()).unwrap();
    done.recv().await.unwrap();
    stopped_promptly.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        !state.snapshot().available,
        "Late collection must not publish after stop"
    );
}

#[tokio::test]
async fn already_stopped_never_starts_collection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let (_stop, stopped) = watch::channel(true);
    sample_loop_with_period(
        SampleState::default(),
        move || {
            attempts.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>(sample())
        },
        stopped,
        Duration::from_millis(10),
        Duration::from_millis(20),
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
