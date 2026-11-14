//! Background queue scheduler worker.

use odm_core::engine::EngineError;
use odm_core::schedule::{Clock, SystemClock};
use odm_core::{Engine, JobId, JobState};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Background queue scheduler evaluating start and stop times.
pub struct Scheduler {
    engine: Engine,
    clock: Arc<dyn Clock>,
    running: Arc<AtomicBool>,
    /// Jobs this scheduler paused at a stop time; only these resume at the next start.
    paused_by_schedule: Mutex<HashSet<JobId>>,
}

impl Scheduler {
    /// Creates a new scheduler with the default SystemClock.
    pub fn new(engine: Engine) -> Self {
        Self::new_with_clock(engine, Arc::new(SystemClock))
    }

    /// Creates a scheduler with an injectable clock (for testing).
    pub fn new_with_clock(engine: Engine, clock: Arc<dyn Clock>) -> Self {
        Self {
            engine,
            clock,
            running: Arc::new(AtomicBool::new(false)),
            paused_by_schedule: Mutex::new(HashSet::new()),
        }
    }

    /// Performs one schedule evaluation tick across all queues.
    pub async fn tick(&self) {
        let (day, time) = self.clock.now();
        let queues = match self.engine.store().list_queues() {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!("Scheduler failed to list queues: {e}");
                return;
            }
        };

        for queue in queues {
            let Some(schedule) = &queue.schedule else {
                continue;
            };

            if schedule.should_start(&day, &time) {
                tracing::info!(
                    queue = %queue.name,
                    time = %time,
                    day = %day,
                    "Starting queue on schedule"
                );
                self.requeue_jobs_paused_by_schedule(queue.id);
                let engine = self.engine.clone();
                let queue_name = queue.name.clone();
                tokio::spawn(async move {
                    if let Err(e) = engine.run_queue(&queue_name).await {
                        tracing::warn!(queue = %queue_name, "Scheduled run_queue error: {e}");
                    }
                });
            }

            if schedule.should_stop(&day, &time) {
                tracing::info!(
                    queue = %queue.name,
                    time = %time,
                    day = %day,
                    "Stopping queue on schedule"
                );
                if let Ok(jobs) = self.engine.store().queue_jobs(queue.id) {
                    for job in jobs {
                        if matches!(
                            job.state(),
                            JobState::Downloading | JobState::Probing | JobState::Verifying
                        ) {
                            let paused = match self.engine.pause(job.id()) {
                                Ok(()) => true,
                                Err(EngineError::NotRunning(_)) => {
                                    let mut updated = job.clone();
                                    updated.transition_to(JobState::Paused).is_ok()
                                        && self.engine.store().update_job(&updated).is_ok()
                                }
                                Err(_) => false,
                            };
                            if paused {
                                self.paused_by_schedule
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .insert(job.id());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Puts the jobs this scheduler paused in the queue back to `Queued`, so the
    /// queue run picks them up. Jobs the user paused or resumed are left alone.
    fn requeue_jobs_paused_by_schedule(&self, queue_id: i64) {
        let Ok(jobs) = self.engine.store().queue_jobs(queue_id) else {
            return;
        };
        let mut paused = self
            .paused_by_schedule
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        for job in jobs {
            if !paused.remove(&job.id()) || job.state() != JobState::Paused {
                continue;
            }
            let mut queued = job;
            if queued.transition_to(JobState::Queued).is_ok() {
                let _ = self.engine.store().update_job(&queued);
            }
        }
    }

    /// Starts the background scheduler loop.
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        self.running.store(true, Ordering::SeqCst);
        let scheduler = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            let mut last_minute: Option<String> = None;
            while scheduler.running.load(Ordering::SeqCst) {
                interval.tick().await;
                let (_, time) = scheduler.clock.now();
                if last_minute.as_deref() != Some(&time) {
                    last_minute = Some(time);
                    scheduler.tick().await;
                }
            }
        })
    }

    /// Stops the background scheduler loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
