use board_observe::ResourceSample;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, watch};

pub const SAMPLE_PERIOD: Duration = Duration::from_secs(5);
const SAMPLE_DEADLINE: Duration = Duration::from_secs(2);
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(30);

#[derive(Clone, Default)]
pub struct SampleState(Arc<Mutex<Record>>);

#[derive(Default)]
struct Record {
    sample: ResourceSample,
    succeeded: Option<Instant>,
}

impl SampleState {
    pub fn snapshot(&self) -> ResourceSample {
        self.snapshot_at(Instant::now())
    }

    pub(crate) fn snapshot_at(&self, now: Instant) -> ResourceSample {
        let Ok(record) = self.0.lock() else {
            return ResourceSample::default();
        };
        let mut sample = record.sample;
        sample.available &= record
            .succeeded
            .is_some_and(|at| now.saturating_duration_since(at) <= MAX_SAMPLE_AGE);
        sample
    }

    pub(crate) fn failure(&self) {
        if let Ok(mut record) = self.0.lock() {
            record.sample.available = false;
        }
    }

    pub(crate) fn success_at(&self, mut sample: ResourceSample, at: Instant, timestamp: u64) {
        if !sample.available
            || sample.storages.iter().all(Option::is_none)
            || sample.services.iter().all(Option::is_none)
        {
            self.failure();
            return;
        }
        sample.last_success_timestamp_seconds = timestamp;
        if let Ok(mut record) = self.0.lock() {
            record.sample = sample;
            record.succeeded = Some(at);
        }
    }
}

pub async fn sample_loop<F, E>(state: SampleState, collect: F, stopped: watch::Receiver<bool>)
where
    F: Fn() -> Result<ResourceSample, E> + Send + Sync + 'static,
    E: Send + 'static,
{
    sample_loop_with_period(state, collect, stopped, SAMPLE_PERIOD, SAMPLE_DEADLINE).await;
}

pub(crate) async fn sample_loop_with_period<F, E>(
    state: SampleState,
    collect: F,
    mut stopped: watch::Receiver<bool>,
    period: Duration,
    deadline: Duration,
) where
    F: Fn() -> Result<ResourceSample, E> + Send + Sync + 'static,
    E: Send + 'static,
{
    let collect = Arc::new(collect);
    let admission = Arc::new(Semaphore::new(1));
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
        let Ok(permit) = admission.clone().try_acquire_owned() else {
            state.failure();
            continue;
        };
        let collector = collect.clone();
        let started = Instant::now();
        let mut operation = tokio::task::spawn_blocking(move || {
            // The blocking operation, not its awaiting request, owns admission.
            // Timeout/drop cannot allow another blocked filesystem operation.
            let _permit = permit;
            collector()
        });
        let result = tokio::select! {
            biased;
            _ = stopped.changed() => return,
            result = tokio::time::timeout(deadline, &mut operation) => result,
        };
        match result {
            Ok(Ok(Ok(sample))) if started.elapsed() <= deadline => {
                match SystemTime::now().duration_since(UNIX_EPOCH) {
                    Ok(timestamp) => state.success_at(sample, Instant::now(), timestamp.as_secs()),
                    Err(_) => state.failure(),
                }
            }
            _ => state.failure(),
        }
    }
}
