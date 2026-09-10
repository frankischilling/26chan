use crate::{config::Targets, journal::unavailable};
use board_observe::MaintenanceSample;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, watch};

pub const SAMPLE_PERIOD: Duration = Duration::from_secs(5);
const SAMPLE_DEADLINE: Duration = Duration::from_secs(2);
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct SampleState {
    configured: Arc<MaintenanceSample>,
    record: Arc<Mutex<Record>>,
}

struct Record {
    sample: MaintenanceSample,
    observed: [Option<Instant>; 4],
}

impl SampleState {
    pub fn new(targets: &Targets) -> Self {
        let sample = unavailable(targets);
        Self {
            configured: Arc::new(sample),
            record: Arc::new(Mutex::new(Record {
                sample,
                observed: [None; 4],
            })),
        }
    }

    pub fn snapshot(&self) -> MaintenanceSample {
        self.snapshot_at(Instant::now())
    }

    pub fn ready(&self) -> bool {
        let sample = self.snapshot();
        sample.targets.iter().any(Option::is_some)
            && sample
                .targets
                .iter()
                .all(|target| target.is_none_or(|target| target.available))
    }

    pub(crate) fn snapshot_at(&self, now: Instant) -> MaintenanceSample {
        let Ok(record) = self.record.lock() else {
            return *self.configured;
        };
        let mut sample = record.sample;
        for (index, entry) in sample.targets.iter_mut().enumerate() {
            if let Some(entry) = entry
                && record.observed[index]
                    .is_none_or(|at| now.saturating_duration_since(at) > MAX_SAMPLE_AGE)
            {
                let mut missing = self.configured.targets[index].expect("configured cache slot");
                missing.last_sample_timestamp_seconds = entry.last_sample_timestamp_seconds;
                *entry = missing;
            }
        }
        sample
    }

    pub(crate) fn failure(&self) {
        if let Ok(mut record) = self.record.lock() {
            for (entry, configured) in record
                .sample
                .targets
                .iter_mut()
                .zip(&self.configured.targets)
            {
                if let (Some(previous), Some(mut missing)) = (*entry, *configured) {
                    missing.last_sample_timestamp_seconds = previous.last_sample_timestamp_seconds;
                    *entry = Some(missing);
                }
            }
        }
    }

    pub(crate) fn publish_at(&self, sample: MaintenanceSample, at: Instant, timestamp: u64) {
        if timestamp == 0 {
            self.failure();
            return;
        }
        if let Ok(mut record) = self.record.lock() {
            for (index, configured) in self.configured.targets.iter().enumerate() {
                let Some(mut configured) = *configured else {
                    continue;
                };
                if let Some(mut observed) = sample.targets[index].filter(|value| value.available) {
                    observed.last_sample_timestamp_seconds = timestamp;
                    observed.max_age_seconds = configured.max_age_seconds;
                    observed.run_timeout_seconds = configured.run_timeout_seconds;
                    record.sample.targets[index] = Some(observed);
                    record.observed[index] = Some(at);
                } else {
                    configured.last_sample_timestamp_seconds = record.sample.targets[index]
                        .map_or(0, |value| value.last_sample_timestamp_seconds);
                    record.sample.targets[index] = Some(configured);
                }
            }
        }
    }
}

pub async fn sample_loop<F>(state: SampleState, collect: F, stopped: watch::Receiver<bool>)
where
    F: Fn() -> MaintenanceSample + Send + Sync + 'static,
{
    sample_loop_with_period(state, collect, stopped, SAMPLE_PERIOD, SAMPLE_DEADLINE).await;
}

pub(crate) async fn sample_loop_with_period<F>(
    state: SampleState,
    collect: F,
    mut stopped: watch::Receiver<bool>,
    period: Duration,
    deadline: Duration,
) where
    F: Fn() -> MaintenanceSample + Send + Sync + 'static,
{
    let collector = Arc::new(collect);
    let admission = Arc::new(Semaphore::new(1));
    let mut ticks = tokio::time::interval(period);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *stopped.borrow() {
            return;
        }
        tokio::select! { biased; _ = stopped.changed() => return, _ = ticks.tick() => (), }
        let Ok(permit) = admission.clone().try_acquire_owned() else {
            state.failure();
            continue;
        };
        let collector = collector.clone();
        let started = Instant::now();
        let mut operation = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            collector()
        });
        let result = tokio::select! { biased;
            _ = stopped.changed() => return,
            result = tokio::time::timeout(deadline,&mut operation) => result,
        };
        match result {
            Ok(Ok(sample)) if started.elapsed() <= deadline => {
                match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(timestamp) => state.publish_at(sample, Instant::now(), timestamp.as_secs()),
                    Err(_) => state.failure(),
                }
            }
            _ => state.failure(),
        }
    }
}
