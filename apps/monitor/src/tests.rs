use super::*;
use board_store::monitoring::QueueSnapshot;
use std::time::{Duration, Instant};

fn sample() -> QueueSnapshot {
    QueueSnapshot {
        capacity: 4,
        receiving: 1,
        queued: 2,
        processing: 1,
        expired_receiving: 0,
        expired_queued: 1,
        expired_processing: 0,
        oldest_queued_seconds: 20,
        intake_failed: 0,
        abandoned: 0,
        processing_failed: 1,
        invalid_output: 0,
        retry_exhausted: 0,
    }
}

#[test]
fn initial_failed_stale_invalid_and_recovered_samples_are_distinct() {
    let state = SampleState::default();
    let now = Instant::now();
    assert!(!state.snapshot_at(now).available);
    state.success_at(sample(), now, 42);
    let fresh = state.snapshot_at(now);
    assert!(fresh.available);
    assert_eq!(fresh.active, [1, 2, 1]);
    assert_eq!(fresh.failures_recent, [0, 0, 1, 0, 0]);
    assert_eq!(fresh.last_success_timestamp_seconds, 42);
    assert!(!state.snapshot_at(now + Duration::from_secs(31)).available);
    state.failure();
    assert!(!state.snapshot_at(now).available);
    assert_eq!(state.snapshot_at(now).last_success_timestamp_seconds, 42);
    let mut invalid = sample();
    invalid.capacity = 0;
    state.success_at(invalid, now, 50);
    assert!(!state.snapshot_at(now).available);
    assert_eq!(state.snapshot_at(now).last_success_timestamp_seconds, 42);
    state.success_at(sample(), now, 60);
    assert!(state.snapshot_at(now).available);
}

#[tokio::test]
async fn sampling_never_overlaps_and_shutdown_cancels_a_pending_query() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    struct PendingGuard(Arc<AtomicUsize>);
    impl Drop for PendingGuard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let started = Arc::new(tokio::sync::Notify::new());
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let state = SampleState::default();
    let observed = state.clone();
    let work_calls = calls.clone();
    let work_started = started.clone();
    let work_dropped = dropped.clone();
    let task = tokio::spawn(sample_loop_with_period(
        state,
        move || {
            work_calls.fetch_add(1, Ordering::SeqCst);
            work_started.notify_one();
            let guard = PendingGuard(work_dropped.clone());
            async move {
                let _guard = guard;
                std::future::pending::<Result<QueueSnapshot, ()>>().await
            }
        },
        stopped,
        Duration::from_millis(10),
    ));
    tokio::time::timeout(Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!observed.snapshot().available);
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_delayed_query_skips_missed_ticks_without_a_catch_up_burst() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let work_calls = calls.clone();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(sample_loop_with_period(
        SampleState::default(),
        move || {
            let first = work_calls.fetch_add(1, Ordering::SeqCst) == 0;
            async move {
                if first {
                    tokio::time::sleep(Duration::from_millis(35)).await;
                }
                Ok::<_, ()>(sample())
            }
        },
        stopped,
        Duration::from_millis(10),
    ));
    tokio::task::yield_now().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_millis(35)).await;
    for _ in 0..3 {
        tokio::task::yield_now().await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    stop.send(true).unwrap();
    task.await.unwrap();
}
