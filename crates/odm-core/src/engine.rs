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
use crate::filename::sanitize_filename;
use crate::http::HttpBackend;
use crate::job::{BackendKind, Job, JobId, JobOptions, JobState, Source, TransitionError};
use crate::limiter::{Limiter, TokenBucket};
use crate::store::{Store, StoreError};

/// How long `remove` waits for a running job to stop.
const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// How often a running job's byte count is written to the store.
const PROGRESS_SAVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

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

/// Values a caller chooses for a new job instead of the engine's defaults.
#[derive(Debug, Clone, Default)]
pub struct AddOverrides {
    pub filename: Option<String>,
    pub category: Option<String>,
    pub queue_id: Option<i64>,
    pub backend: Option<BackendKind>,
}

/// Coordinates probing, downloading and persisting one job at a time.
#[derive(Clone)]
pub struct Engine {
    store: Store,
    backends: Arc<Mutex<HashMap<BackendKind, Arc<dyn Backend>>>>,
    limiter: Arc<TokenBucket>,
    controls: Arc<Mutex<HashMap<JobId, ControlHandle>>>,
    events: broadcast::Sender<Event>,
}

impl Engine {
    pub fn new(store: Store) -> Result<Self, EngineError> {
        store.recover_interrupted()?;
        let (events, _) = broadcast::channel(1024);
        let http_backend = Arc::new(HttpBackend::new()?);
        let mut backends = HashMap::new();
        backends.insert(BackendKind::Http, http_backend as Arc<dyn Backend>);
        Ok(Self {
            store,
            backends: Arc::new(Mutex::new(backends)),
            limiter: Arc::new(TokenBucket::unlimited()),
            controls: Arc::new(Mutex::new(HashMap::new())),
            events,
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Moves a job within its queue and announces the queue change.
    pub fn set_job_position(&self, id: JobId, position: i64) -> Result<(), EngineError> {
        self.store.set_job_position(id, position)?;
        let queue_id = self.store.get_job(id)?.queue_id();
        if let Some(queue) = self
            .store
            .list_queues()?
            .into_iter()
            .find(|queue| queue.id == queue_id)
        {
            self.announce_queue_changed(&queue.name);
        }
        Ok(())
    }

    /// Tells subscribers that a queue's order, limit or existence changed.
    pub fn announce_queue_changed(&self, name: &str) {
        let _ = self.events.send(Event::QueueChanged(name.to_string()));
    }

    /// Sets the speed limit shared by all downloads; `None` or zero removes it.
    pub fn set_global_speed_limit(&self, bytes_per_second: Option<u64>) {
        self.limiter.set_rate(bytes_per_second);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn events(&self) -> broadcast::Sender<Event> {
        self.events.clone()
    }

    /// Registers a backend implementation for its kind.
    pub fn register_backend(&self, backend: Arc<dyn Backend>) {
        let mut map = self.backends.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(backend.kind(), backend);
    }

    /// Returns the registered backend for a given kind.
    pub fn get_backend(&self, kind: BackendKind) -> Result<Arc<dyn Backend>, EngineError> {
        let map = self.backends.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&kind).cloned().ok_or(EngineError::NoBackend(kind))
    }

    /// Adds a download job of any supported source kind to the store.
    pub fn add_download(
        &self,
        source: Source,
        target_dir: PathBuf,
        options: JobOptions,
    ) -> Result<Job, EngineError> {
        self.add_download_with(source, target_dir, options, AddOverrides::default())
    }

    /// Adds a job with the given overrides applied before it is stored, so the row and
    /// the `JobAdded` event already carry the final filename, category, queue and backend.
    pub fn add_download_with(
        &self,
        source: Source,
        target_dir: PathBuf,
        options: JobOptions,
        overrides: AddOverrides,
    ) -> Result<Job, EngineError> {
        let backend_kind = match &source {
            Source::Url(url_str) => {
                let url = Url::parse(url_str)
                    .map_err(|error| EngineError::InvalidUrl(error.to_string()))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(EngineError::InvalidUrl(url.to_string()));
                }
                match url.path().rsplit('.').next() {
                    Some("m3u8") => BackendKind::Hls,
                    Some("mpd") => BackendKind::Dash,
                    _ => BackendKind::Http,
                }
            }
            Source::Magnet(_) | Source::TorrentFile(_) => BackendKind::Torrent,
        };

        let mut job = Job::new(source, target_dir, options);
        job.set_backend(overrides.backend.unwrap_or(backend_kind));
        if let Some(filename) = overrides.filename {
            job.set_filename(filename);
        }
        if let Some(category) = overrides.category {
            job.set_category(Some(category));
        }
        if let Some(queue_id) = overrides.queue_id {
            job.set_queue_id(queue_id);
        }
        self.store.insert_job(&mut job)?;
        let _ = self.events.send(Event::JobAdded(job.id()));
        Ok(job)
    }

    /// Adds an HTTP job to the store.
    pub fn add(
        &self,
        url: &str,
        target_dir: PathBuf,
        options: JobOptions,
    ) -> Result<Job, EngineError> {
        self.add_download(Source::Url(url.to_string()), target_dir, options)
    }

    /// Removes a job from the store, cancels it if running, and optionally deletes the file.
    /// A running job is given time to stop first, so it cannot write the file again.
    pub async fn remove(&self, id: JobId, delete_file: bool) -> Result<(), EngineError> {
        let _ = self.cancel(id);
        self.wait_until_idle(id).await;
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

    /// Pauses a running job, or a queued job that has not started yet.
    pub fn pause(&self, id: JobId) -> Result<(), EngineError> {
        {
            let map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(handle) = map.get(&id) {
                handle.pause();
                return Ok(());
            }
        }
        let mut job = self.store.get_job(id)?;
        if job.state() != JobState::Queued {
            return Err(EngineError::NotRunning(id));
        }
        job.transition_to(JobState::Paused)?;
        self.store.update_job(&job)?;
        let _ = self.events.send(Event::JobStateChanged {
            id,
            state: JobState::Paused,
        });
        Ok(())
    }

    /// Waits until no run of the job is active, for at most `STOP_TIMEOUT`.
    async fn wait_until_idle(&self, id: JobId) {
        self.wait_while(STOP_TIMEOUT, |controls| controls.contains_key(&id))
            .await;
    }

    /// Polls the active runs until `busy` is false or `limit` has passed.
    async fn wait_while(
        &self,
        limit: std::time::Duration,
        busy: impl Fn(&HashMap<JobId, ControlHandle>) -> bool,
    ) {
        let idle = async {
            while busy(&self.controls.lock().unwrap_or_else(|p| p.into_inner())) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        let _ = tokio::time::timeout(limit, idle).await;
    }

    /// Pauses every running job and waits up to `limit` for them to stop.
    pub async fn pause_all(&self, limit: std::time::Duration) {
        {
            let map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
            for handle in map.values() {
                handle.pause();
            }
        }
        self.wait_while(limit, |controls| !controls.is_empty())
            .await;
    }

    /// Deletes the partial file and saved segments of a job that will not resume.
    fn discard_partial(&self, job: &Job) {
        let _ = std::fs::remove_file(job.part_path());
        let _ = self.store.clear_segments(job.id());
    }

    /// Cancels a running or queued job. A running job finishes cancelling in the
    /// background; a job that is not running is cancelled and cleaned up here.
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
            self.discard_partial(&job);
            Ok(())
        }
    }

    /// Records a failure: stores the error, moves the job to `Failed` and announces it.
    fn fail_job(&self, job: &mut Job, message: String, retryable: bool) -> Result<(), EngineError> {
        job.set_error(message, retryable);
        job.transition_to(JobState::Failed)?;
        self.store.update_job(job)?;
        let _ = self.events.send(Event::JobStateChanged {
            id: job.id(),
            state: JobState::Failed,
        });
        Ok(())
    }

    /// Probes, downloads and persists one job until it finishes or fails.
    pub async fn run(&self, id: JobId) -> Result<Job, EngineError> {
        // Claim the job first, so two runs can never work on it together.
        let (control_handle, control) = ControlToken::new();
        {
            let mut map = self.controls.lock().unwrap_or_else(|p| p.into_inner());
            if map.contains_key(&id) {
                return Err(EngineError::AlreadyRunning(id));
            }
            map.insert(id, control_handle);
        }
        let _guard = ControlGuard {
            id,
            controls: self.controls.clone(),
        };

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

        let has_segments = !self.store.list_segments(id)?.is_empty();
        if !has_segments {
            job.transition_to(JobState::Probing)?;
            self.store.update_job(&job)?;
            let _ = self.events.send(Event::JobStateChanged {
                id,
                state: JobState::Probing,
            });

            let backend = match self.get_backend(job.backend()) {
                Ok(backend) => backend,
                Err(error) => {
                    self.fail_job(&mut job, error.to_string(), false)?;
                    return Err(error);
                }
            };
            let probe = match backend.probe(job.source(), job.options()).await {
                Ok(probe) => probe,
                Err(error) => {
                    self.fail_job(&mut job, error.to_string(), error.is_retryable())?;
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

            // A name the job already has (set by the user or an earlier run) is ours:
            // only other jobs and existing finished files conflict with it.
            let has_own_name = !job.filename().is_empty();
            let wanted = if has_own_name {
                job.filename().to_string()
            } else {
                probe.filename.clone()
            };
            let wanted = sanitize_filename(&wanted).unwrap_or_else(|| "download".to_string());
            job.set_filename(resolve_filename_conflict(
                job.target_dir(),
                &wanted,
                &taken,
                has_own_name,
            ));

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
        let progress_store = self.store.clone();
        let total_bytes = job.total_bytes();
        let initial_bytes = job.downloaded_bytes();

        let progress_task = tokio::spawn(async move {
            let mut last_emit = tokio::time::Instant::now();
            let mut last_saved: Option<tokio::time::Instant> = None;
            let mut last_bytes = initial_bytes;
            let mut final_bytes = initial_bytes;
            while let Some(update) = updates.recv().await {
                final_bytes = update.downloaded_bytes;
                if let Some(state) = update.state {
                    let _ = events_tx.send(Event::JobStateChanged { id, state });
                }
                let now = tokio::time::Instant::now();
                // Keep the stored count fresh, so a crash loses at most a second of it.
                if last_saved
                    .is_none_or(|saved| now.duration_since(saved) >= PROGRESS_SAVE_INTERVAL)
                {
                    let _ = progress_store.save_downloaded_bytes(id, final_bytes);
                    last_saved = Some(now);
                }
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

        let backend = match self.get_backend(job.backend()) {
            Ok(backend) => backend,
            Err(error) => {
                progress_task.abort();
                self.fail_job(&mut job, error.to_string(), false)?;
                return Err(error);
            }
        };
        let outcome = match backend.run(context).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = progress_task.await;
                self.fail_job(&mut job, error.to_string(), error.is_retryable())?;
                return Err(error.into());
            }
        };

        let downloaded_bytes = progress_task.await.unwrap_or(job.downloaded_bytes());
        job.set_downloaded_bytes(downloaded_bytes);

        match outcome {
            RunOutcome::Completed => {
                if let Some(checksum) = job.options().checksum.clone() {
                    job.transition_to(JobState::Verifying)?;
                    self.store.update_job(&job)?;
                    let _ = self.events.send(Event::JobStateChanged {
                        id,
                        state: JobState::Verifying,
                    });

                    if let Err(err) =
                        crate::checksum::verify_file_checksum(&job.target_path(), &checksum).await
                    {
                        self.fail_job(&mut job, err.to_string(), false)?;
                        return Err(EngineError::Checksum(err));
                    }
                }
                job.transition_to(JobState::Completed)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Completed,
                });
            }
            RunOutcome::Paused => {
                job.transition_to(JobState::Paused)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Paused,
                });
            }
            RunOutcome::Cancelled => {
                self.discard_partial(&job);
                job.transition_to(JobState::Cancelled)?;
                self.store.update_job(&job)?;
                let _ = self.events.send(Event::JobStateChanged {
                    id,
                    state: JobState::Cancelled,
                });
            }
        }
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
    #[error("checksum error: {0}")]
    Checksum(#[from] crate::checksum::ChecksumError),
    #[error("join error: {0}")]
    Join(String),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("job is not runnable in state {0:?}")]
    InvalidState(JobState),
    #[error("job {0:?} is not currently running")]
    NotRunning(JobId),
    #[error("job {0:?} is already running")]
    AlreadyRunning(JobId),
    #[error("no backend registered for {0:?}")]
    NoBackend(BackendKind),
}

fn resolve_filename_conflict(
    target_dir: &std::path::Path,
    filename: &str,
    taken_names: &[String],
    own_part_file_ok: bool,
) -> String {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (filename, None),
    };

    let is_taken = |candidate: &str| -> bool {
        target_dir.join(candidate).exists()
            || (!own_part_file_ok && target_dir.join(format!("{candidate}.part")).exists())
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
