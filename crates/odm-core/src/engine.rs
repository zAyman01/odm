//! Job orchestration: add, probe, download and persist.

use std::path::PathBuf;

use url::Url;

use crate::backend::{Backend, BackendError, ControlToken, JobContext, ProgressSink, RunOutcome};
use crate::http::HttpBackend;
use crate::job::{BackendKind, Job, JobId, JobOptions, JobState, Source, TransitionError};
use crate::store::{Store, StoreError};

/// Coordinates probing, downloading and persisting one job at a time.
pub struct Engine {
    store: Store,
    backend: HttpBackend,
}

impl Engine {
    pub fn new(store: Store) -> Result<Self, EngineError> {
        Ok(Self {
            store,
            backend: HttpBackend::new()?,
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

    /// Probes, downloads and persists one job until it finishes or fails.
    pub async fn run(&self, id: JobId) -> Result<Job, EngineError> {
        let mut job = self.store.get_job(id)?;
        if job.state() == JobState::Failed {
            job.transition_to(JobState::Queued)?;
        }
        if job.state() != JobState::Queued {
            return Err(EngineError::InvalidState(job.state()));
        }

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
        job.transition_to(JobState::Downloading)?;
        self.store.update_job(&job)?;

        let (_control_handle, control) = ControlToken::new();
        let (progress, mut updates) = ProgressSink::new();
        let context = JobContext {
            job: job.clone(),
            control,
            progress,
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
}
