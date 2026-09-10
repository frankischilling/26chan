#![forbid(unsafe_code)]

use board_observe::MediaQueueSample;
use board_store::monitoring::QueueSnapshot;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub const SAMPLE_PERIOD: Duration = Duration::from_secs(5);
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct SampleState(Arc<Mutex<Record>>);

#[derive(Default)]
struct Record {
    sample: MediaQueueSample,
    succeeded: Option<Instant>,
}

impl SampleState {
    pub fn snapshot(&self) -> MediaQueueSample {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> MediaQueueSample {
        let Ok(record) = self.0.lock() else {
            return MediaQueueSample::default();
        };
        let mut sample = record.sample;
        sample.available &= record
            .succeeded
            .is_some_and(|at| now.saturating_duration_since(at) <= MAX_SAMPLE_AGE);
        sample
    }

    fn failure(&self) {
        if let Ok(mut record) = self.0.lock() {
            record.sample.available = false;
        }
    }

    fn success_at(&self, value: QueueSnapshot, at: Instant, timestamp: u64) {
        let counts = [
            value.receiving,
            value.queued,
            value.processing,
            value.expired_receiving,
            value.expired_queued,
            value.expired_processing,
            value.oldest_queued_seconds,
            value.intake_failed,
            value.abandoned,
            value.processing_failed,
            value.invalid_output,
            value.retry_exhausted,
        ];
        if !(1..=1024).contains(&value.capacity)
            || counts.iter().any(|count| *count < 0)
            || value.expired_receiving > value.receiving
            || value.expired_queued > value.queued
            || value.expired_processing > value.processing
        {
            self.failure();
            return;
        }
        if let Ok(mut record) = self.0.lock() {
            record.sample = MediaQueueSample {
                available: true,
                last_success_timestamp_seconds: timestamp,
                capacity: value.capacity as u64,
                active: [
                    value.receiving as u64,
                    value.queued as u64,
                    value.processing as u64,
                ],
                expired: [
                    value.expired_receiving as u64,
                    value.expired_queued as u64,
                    value.expired_processing as u64,
                ],
                oldest_queued_seconds: value.oldest_queued_seconds as u64,
                failures_recent: [
                    value.intake_failed as u64,
                    value.abandoned as u64,
                    value.processing_failed as u64,
                    value.invalid_output as u64,
                    value.retry_exhausted as u64,
                ],
            };
            record.succeeded = Some(at);
        }
    }
}

pub async fn sample_loop<F, Q, E>(state: SampleState, query: Q, stopped: watch::Receiver<bool>)
where
    Q: FnMut() -> F,
    F: Future<Output = Result<QueueSnapshot, E>>,
{
    sample_loop_with_period(state, query, stopped, SAMPLE_PERIOD).await;
}

async fn sample_loop_with_period<F, Q, E>(
    state: SampleState,
    mut query: Q,
    mut stopped: watch::Receiver<bool>,
    period: Duration,
) where
    Q: FnMut() -> F,
    F: Future<Output = Result<QueueSnapshot, E>>,
{
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *stopped.borrow() {
            return;
        }
        tokio::select! {
            biased;
            _ = stopped.changed() => return,
            _ = ticks.tick() => (),
        }
        let result = tokio::select! {
            biased;
            _ = stopped.changed() => return,
            result = query() => result,
        };
        match result {
            Ok(sample) => {
                if let Ok(timestamp) = SystemTime::now().duration_since(UNIX_EPOCH) {
                    state.success_at(sample, Instant::now(), timestamp.as_secs());
                } else {
                    state.failure();
                }
            }
            Err(_) => state.failure(),
        }
    }
}

#[cfg(test)]
mod tests;
