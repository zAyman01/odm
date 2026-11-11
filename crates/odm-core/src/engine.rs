//! Job orchestration: add, probe, download, pause, cancel and persist.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use tokio::task::JoinSet;
use url::Url;

use crate::backend::{
    Backend, BackendError, ControlHandle, ControlToken, JobContext, ProgressSink, RunOutcome,
};
use crate::event::Event;
use crate::http::HttpBackend;
use crate::job::{BackendKind, Job, JobId, JobOptions, JobState, Source, TransitionError};
use crate::limiter::{Limiter, TokenBucket};
use crate::store::{Store, StoreError};

struct ControlGuard {
    id: JobId,
    controls: Arc<Mutex<HashMap<JobId, ControlHandle>>>,
}

impl Drop for ControlGuard {
    fn drop(&mut self) {
        let mut map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&self.id);
    }
}

/// Coordinates probing, downloading and persisting one job at a time.
#[derive(Clone)]
pub struct Engine {
    store: Store,
    backend: HttpBackend,
    limiter: Arc<TokenBucket>,
    controls: Arc<Mutex<HashMap<JobId, ControlHandle>>>,
    events: broadcast::Sender<Event>,
}

impl Engine {
    pub fn new(store: Store) -> Result<Self, EngineError> {
        store.recover_interrupted()?;
        let (events, _) = broadcast::channel(1024);
        Ok(Self {
            store,
            backend: HttpBackend::new()?,
            limiter: Arc::new(TokenBucket::unlimited()),
            controls: Arc::new(Mutex::new(HashMap::new())),
            events,
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn events(&self) -> broadcast::Sender<Event> {
        self.events.clone()
    }

    /// Adds an HTTP job to the store.
    pub fn add(
        &self,
        url: &str,
        target_dir: PathBuf,
        options: JobOptions,
    ) -> Result<Job, EngineError> {
        let url = Url::parse(url).map_err(|error| EngineError::InvalidUrl(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EngineError::InvalidUrl(url.to_string()));
        }

        let mut job = Job::new(Source::Url(url.to_string()), target_dir, options);
        job.set_backend(BackendKind::Http);
        self.store.insert_job(&mut job)?;
        let _ = self.events.send(Event::JobAdded(job.id()));
        Ok(job)
    }

    /// Removes a job from the store, cancels it if running, and optionally deletes the file.
    pub fn remove(&self, id: JobId, delete_file: bool) -> Result<(), EngineError> {
        let _ = self.cancel(id);
        let job = self.store.get_job(id)?;
        self.store.delete_job(id)?;
        if delete_file {
            let file_path = job.target_dir().join(job.filename());
            let part_path = job.target_dir().join(format!("{}.part", job.filename()));
            let _ = std::fs::remove_file(file_path);
            let _ = std::fs::remove_file(part_path);
        }
        let _ = self.events.send(Event::JobRemoved(id));
        Ok(())
    }

    /// Pauses a running job.
    pub fn pause(&self, id: JobId) -> Result<(), EngineError> {
        let map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(handle) = map.get(&id) {
            handle.pause();
            Ok(())
        } else {
            Err(EngineError::NotRunning(id))
        }
    }

    /// Cancels a running or queued job.
    pub fn cancel(&self, id: JobId) -> Result<(), EngineError> {
        let handle = {
            let map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
            map.get(&id).cloned()
        };

        if let Some(handle) = handle {
            handle.cancel();
            Ok(())
        } else {
            let mut job = self.store.get_job(id)?;
            if matches!(job.state(), JobState::Completed | JobState::Cancelled) {
                return Err(EngineError::InvalidState(job.state()));
            }
            job.transition_to(JobState::Cancelled)?;
            self.store.update_job(&job)?;
            Ok(())
        }
    }

    /// Probes, downloads and persists one job until it finishes or fails.
    pub async fn run(&self, id: JobId) -> Result<Job, EngineError> {
        let mut job = self.store.get_job(id)?;
        if job.state() == JobState::Paused {
            job.transition_to(JobState::Queued)?;
            self.store.update_job(&job)?;
        } else if job.state() == JobState::Failed {
            job.clear_error();
            job.transition_to(JobState::Queued)?;
            self.store.update_job(&job)?;
        }

        if job.state() != JobState::Queued {
            return Err(EngineError::InvalidState(job.state()));
        }

        let (control_handle, control) = ControlToken::new();
        {
            let mut map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
            map.insert(id, control_handle);
        }
        let _guard = ControlGuard {
            id,
            controls: self.controls.clone(),
        };

        let has_segments = !self.store.list_segments(id)?.is_empty();
        if !has_segments {
            job.transition_to(JobState::Probing)?;
            self.store.update_job(&job)?;
            let _ = self.events.send(Event::JobStateChanged {
                id,
                state: JobState::Probing,
            });

            let probe = match self.backend.probe(job.source(), job.options()).await {
                Ok(probe) => probe,
                Err(error) => {
                    job.set_error(error.to_string(), error.is_retryable());
                    job.transition_to(JobState::Failed)?;
                    self.store.update_job(&job)?;
                    let _ = self.events.send(Event::JobStateChanged {
                        id,
                        state: JobState::Failed,
                    });
                    return Err(error.into());
                }
            };

            let other_jobs = self.store.list_jobs()?;
            let taken: Vec<String> = other_jobs
                .into_iter()
                .filter(|other| other.id() != id && other.target_dir() == job.target_dir())
                .map(|other| other.filename().to_string())
                .filter(|name| !name.is_empty())
                .collect();

            let mut probe = probe;
            probe.filename = resolve_filename_conflict(job.target_dir(), &probe.filename, &taken);

            job.apply_probe(probe);

            if control.is_cancelled() {
                job.transition_to(JobState::Cancelled)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Cancelled,
                });
                return Ok(job);
            }
            if control.is_paused() {
                job.transition_to(JobState::Paused)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Paused,
                });
                return Ok(job);
            }
        }

        job.transition_to(JobState::Downloading)?;
        self.store.update_job(&job)?;
        let _ = self.events.send(Event::JobStateChanged {
            id,
            state: JobState::Downloading,
        });

        let (progress, mut updates) = ProgressSink::new();
        let context = JobContext {
            job: job.clone(),
            control,
            progress,
            store: self.store.clone(),
            limiter: Limiter::new(self.limiter.clone(), job.options().speed_limit),
        };

        let events_tx = self.events.clone();
        let total_bytes = job.total_bytes();
        let initial_bytes = job.downloaded_bytes();

        let progress_task = tokio::spawn(async move {
            let mut last_emit = tokio::time::Instant::now();
            let mut last_bytes = initial_bytes;
            let mut final_bytes = initial_bytes;
            while let Some(update) = updates.recv().await {
                final_bytes = update.downloaded_bytes;
                let now = tokio::time::Instant::now();
                let elapsed = now.duration_since(last_emit);
                if elapsed >= std::time::Duration::from_millis(250) {
                    let bytes_diff = final_bytes.saturating_sub(last_bytes);
                    let speed_bps = if elapsed.as_secs_f64() > 0.0 {
                        (bytes_diff as f64 / elapsed.as_secs_f64()) as u64
                    } else {
                        0
                    };
                    let _ = events_tx.send(Event::JobProgress {
                        id,
                        downloaded_bytes: final_bytes,
                        total_bytes: update.total_bytes.or(total_bytes),
                        speed_bps,
                    });
                    last_emit = now;
                    last_bytes = final_bytes;
                }
            }
            final_bytes
        });

        let outcome = match self.backend.run(context).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = progress_task.await;
                job.set_error(error.to_string(), error.is_retryable());
                job.transition_to(JobState::Failed)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Failed,
                });
                return Err(error.into());
            }
        };

        let downloaded_bytes = progress_task.await.unwrap_or(job.downloaded_bytes());
        job.set_downloaded_bytes(downloaded_bytes);

        let next = match outcome {
            RunOutcome::Completed => JobState::Completed,
            RunOutcome::Paused => JobState::Paused,
            RunOutcome::Cancelled => JobState::Cancelled,
        };
        job.transition_to(next)?;
        self.store.update_job(&job)?;
        let _ = self.events.send(Event::JobStateChanged { id, state: next });
        Ok(job)
    }

    /// Runs all queued jobs in a queue with bounded concurrency.
    pub async fn run_queue(
        &self,
        name: &str,
    ) -> Result<Vec<Result<Job, EngineError>>, EngineError> {
        let queue = self.store.queue_by_name(name)?;
        let jobs = self.store.queue_jobs(queue.id)?;
        let queued_ids: Vec<JobId> = jobs
            .into_iter()
            .filter(|j| j.state() == JobState::Queued)
            .map(|j| j.id())
            .collect();

        let mut results = Vec::new();
        if queued_ids.is_empty() {
            return Ok(results);
        }

        let max_concurrent = queue.max_concurrent.max(1) as usize;
        let mut join_set = JoinSet::new();
        let mut iter = queued_ids.into_iter();

        for _ in 0..max_concurrent {
            if let Some(id) = iter.next() {
                let engine = self.clone();
                join_set.spawn(async move { engine.run(id).await });
            } else {
                break;
            }
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(run_result) => {
                    results.push(run_result);
                }
                Err(join_err) => {
                    return Err(EngineError::Join(join_err.to_string()));
                }
            }

            if let Some(id) = iter.next() {
                let engine = self.clone();
                join_set.spawn(async move { engine.run(id).await });
            }
        }

        Ok(results)
    }
}

/// Errors returned by the engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("backend error: {0}")]
    Backend(#[from] BackendError),
    #[error("transition error: {0}")]
    Transition(#[from] TransitionError),
    #[error("join error: {0}")]
    Join(String),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("job is not runnable in state {0:?}")]
    InvalidState(JobState),
    #[error("job {0:?} is not currently running")]
    NotRunning(JobId),
}

fn resolve_filename_conflict(
    target_dir: &std::path::Path,
    filename: &str,
    taken_names: &[String],
) -> String {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (filename, None),
    };

    let is_taken = |candidate: &str| -> bool {
        target_dir.join(candidate).exists()
            || target_dir.join(format!("{candidate}.part")).exists()
            || taken_names.iter().any(|name| name == candidate)
    };

    if !is_taken(filename) {
        return filename.to_string();
    }

    let mut counter = 1;
    loop {
        let candidate = match ext {
            Some(ext) => format!("{stem} ({counter}).{ext}"),
            None => format!("{stem} ({counter})"),
        };
        if !is_taken(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}
