//! Job orchestration: add, probe, download, pause, cancel and persist.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::task::JoinSet;
use url::Url;

use crate::backend::{
    Backend, BackendError, ControlHandle, ControlToken, JobContext, ProgressSink, RunOutcome,
};
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
}

impl Engine {
    pub fn new(store: Store) -> Result<Self, EngineError> {
        store.recover_interrupted()?;
        Ok(Self {
            store,
            backend: HttpBackend::new()?,
            limiter: Arc::new(TokenBucket::unlimited()),
            controls: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
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
        Ok(job)
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

            let probe = match self.backend.probe(job.source(), job.options()).await {
                Ok(probe) => probe,
                Err(error) => {
                    job.set_error(error.to_string(), error.is_retryable());
                    job.transition_to(JobState::Failed)?;
                    self.store.update_job(&job)?;
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
                return Ok(job);
            }
            if control.is_paused() {
                job.transition_to(JobState::Paused)?;
                self.store.update_job(&job)?;
                return Ok(job);
            }
        }

        job.transition_to(JobState::Downloading)?;
        self.store.update_job(&job)?;

        let (progress, mut updates) = ProgressSink::new();
        let context = JobContext {
            job: job.clone(),
            control,
            progress,
            store: self.store.clone(),
            limiter: Limiter::new(self.limiter.clone(), job.options().speed_limit),
        };

        let outcome = match self.backend.run(context).await {
            Ok(outcome) => outcome,
            Err(error) => {
                job.set_error(error.to_string(), error.is_retryable());
                job.transition_to(JobState::Failed)?;
                self.store.update_job(&job)?;
                return Err(error.into());
            }
        };

        let mut downloaded_bytes = job.downloaded_bytes();
        while let Some(update) = updates.recv().await {
            downloaded_bytes = update.downloaded_bytes;
        }
        job.set_downloaded_bytes(downloaded_bytes);

        let next = match outcome {
            RunOutcome::Completed => JobState::Completed,
            RunOutcome::Paused => JobState::Paused,
            RunOutcome::Cancelled => JobState::Cancelled,
        };
        job.transition_to(next)?;
        self.store.update_job(&job)?;
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
