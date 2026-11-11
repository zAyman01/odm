//! Job orchestration: add, probe, download, pause, cancel and persist.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("job is not runnable in state {0:?}")]
    InvalidState(JobState),
    #[error("job {0:?} is not currently running")]
    NotRunning(JobId),
}
