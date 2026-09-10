use super::{
    config::{Target, Targets},
    sampler::{SampleState, sample_loop_with_period},
};
use board_observe::{MaintenanceSample, MaintenanceTargetSample};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

fn targets() -> Targets {
    Targets {
        production: false,
        targets: [
            Some(Target {
                path: "application.json".into(),
                max_age_seconds: 60,
                run_timeout_seconds: 10,
            }),
            Some(Target {
                path: "host.json".into(),
                max_age_seconds: 120,
                run_timeout_seconds: 20,
            }),
            None,
            None,
        ],
    }
}
fn available() -> MaintenanceSample {
    MaintenanceSample {
        targets: [
            Some(MaintenanceTargetSample {
                available: true,
                started_ms: 1000,
                last_success_ms: 1500,
                max_age_seconds: 60,
                run_timeout_seconds: 10,
                ..Default::default()
            }),
            Some(MaintenanceTargetSample {
                available: true,
                started_ms: 2000,
                last_success_ms: 2500,
                max_age_seconds: 120,
                run_timeout_seconds: 20,
                ..Default::default()
            }),
            None,
            None,
        ],
    }
}

#[test]
fn cache_starts_with_configured_unavailable_slots_and_keeps_independent_age_and_failure() {
    let state = SampleState::new(&targets());
    let at = Instant::now();
    let initial = state.snapshot_at(at);
    assert!(!state.ready());
    assert!(!initial.targets[0].unwrap().available);
    assert!(!initial.targets[1].unwrap().available);
    assert!(initial.targets[2].is_none());
    state.publish_at(available(), at, 10);
    assert!(state.ready());
    let mut partial = available();
    partial.targets[0].as_mut().unwrap().available = false;
    state.publish_at(partial, at + Duration::from_secs(20), 30);
    let observed = state.snapshot_at(at + Duration::from_secs(30));
    assert!(!observed.targets[0].unwrap().available);
    assert!(observed.targets[1].unwrap().available);
    assert_eq!(
        observed.targets[0].unwrap().last_sample_timestamp_seconds,
        10
    );
    assert_eq!(
        observed.targets[1].unwrap().last_sample_timestamp_seconds,
        30
    );
    assert!(
        state.snapshot_at(at + Duration::from_secs(50)).targets[1]
            .unwrap()
            .available
    );
    assert!(
        !state
            .snapshot_at(at + Duration::from_secs(50) + Duration::from_nanos(1))
            .targets[1]
            .unwrap()
            .available
    );
    state.publish_at(available(), at + Duration::from_secs(51), 61);
    assert!(
        state.snapshot_at(at + Duration::from_secs(51)).targets[0]
            .unwrap()
            .available
    );
}

#[test]
fn recorded_failure_is_ready_missing_targets_are_not_and_unconfigured_data_is_ignored() {
    let state = SampleState::new(&targets());
    let mut sample = available();
    sample.targets[0].as_mut().unwrap().failure_pending = true;
    sample.targets[2] = sample.targets[0];
    state.publish_at(sample, Instant::now(), 10);
    assert!(state.ready());
    assert!(state.snapshot().targets[2].is_none());
    sample.targets[1] = None;
    state.publish_at(sample, Instant::now(), 11);
    assert!(!state.ready());
    assert!(!state.snapshot().targets[1].unwrap().available);
    state.failure();
    assert!(!state.snapshot().targets[0].unwrap().available);
    assert_eq!(
        state.snapshot().targets[0]
            .unwrap()
            .last_sample_timestamp_seconds,
        11
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timed_out_blocking_operation_retains_admission_and_stop_does_not_wait() {
    let state = SampleState::new(&targets());
    let calls = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let counter = calls.clone();
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            entered_tx.send(()).unwrap();
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
            available()
        },
        stop_rx,
        Duration::from_millis(10),
        Duration::from_millis(20),
    ));
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(2)).unwrap())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(70)).await;
    let actual_calls = calls.load(Ordering::SeqCst);
    let ready = state.ready();
    let sent = stop_tx.send(true);
    let ended = tokio::time::timeout(Duration::from_secs(1), task).await;
    let released = release_tx.send(());
    sent.unwrap();
    released.unwrap();
    assert_eq!(actual_calls, 1);
    assert!(!ready);
    ended.unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn already_ready_join_result_after_deadline_is_rejected() {
    let state = SampleState::new(&targets());
    let (started_tx, mut started_rx) = tokio::sync::mpsc::channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            started_tx.blocking_send(()).unwrap();
            std::thread::sleep(Duration::from_millis(30));
            done_tx.send(()).unwrap();
            available()
        },
        stop_rx,
        Duration::from_secs(5),
        Duration::from_millis(10),
    ));
    tokio::time::timeout(Duration::from_secs(2), started_rx.recv())
        .await
        .unwrap()
        .unwrap();
    // Occupy this sole async worker until a completed blocking result is late.
    done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    tokio::task::yield_now().await;
    let ready = state.ready();
    stop_tx.send(true).unwrap();
    task.await.unwrap();
    assert!(!ready);
}

#[tokio::test]
async fn loop_publishes_independent_failure_and_successful_recovery() {
    let state = SampleState::new(&targets());
    let calls = Arc::new(AtomicUsize::new(0));
    let attempts = calls.clone();
    let (fail_tx, fail_rx) = std::sync::mpsc::channel();
    let fail_rx = Mutex::new(fail_rx);
    let (recover_tx, recover_rx) = std::sync::mpsc::channel();
    let recover_rx = Mutex::new(recover_rx);
    let (stop_tx, stop_rx) = watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        state.clone(),
        move || {
            let call = attempts.fetch_add(1, Ordering::SeqCst);
            if call == 1 {
                fail_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                let mut sample = available();
                sample.targets[0].as_mut().unwrap().available = false;
                sample
            } else {
                if call == 2 {
                    recover_rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
                available()
            }
        },
        stop_rx,
        Duration::from_millis(100),
        Duration::from_secs(2),
    ));
    let observed = tokio::time::timeout(Duration::from_secs(3), async {
        while !state.ready() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        fail_tx.send(()).unwrap();
        while calls.load(Ordering::SeqCst) < 2 || state.ready() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let sample = state.snapshot();
        assert!(!sample.targets[0].unwrap().available);
        assert_ne!(sample.targets[0].unwrap().last_sample_timestamp_seconds, 0);
        assert!(sample.targets[1].unwrap().available);
        recover_tx.send(()).unwrap();
        while !state.ready() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    let _ = stop_tx.send(true);
    let _ = fail_tx.send(());
    let _ = recover_tx.send(());
    task.await.unwrap();
    observed.unwrap();
}

#[tokio::test]
async fn already_stopped_does_not_start_collector() {
    let state = SampleState::new(&targets());
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let (_stop, stopped) = watch::channel(true);
    super::sample_loop(
        state,
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
            available()
        },
        stopped,
    )
    .await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
