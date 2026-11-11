//! Domain model for download jobs and the job state machine.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::util::now_millis;

/// Stable identifier of a persisted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub i64);

/// Lifecycle states of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Probing,
    Downloading,
    Paused,
    Verifying,
    Seeding,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    /// Every state, in the order of the design table.
    pub const ALL: [Self; 9] = [
        Self::Queued,
        Self::Probing,
        Self::Downloading,
        Self::Paused,
        Self::Verifying,
        Self::Seeding,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];

    /// Returns whether the design table allows moving from `self` to `next`.
    pub fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Queued => matches!(next, Self::Probing | Self::Paused | Self::Cancelled),
            Self::Probing => matches!(
                next,
                Self::Downloading | Self::Failed | Self::Paused | Self::Cancelled
            ),
            Self::Downloading => matches!(
                next,
                Self::Paused
                    | Self::Verifying
                    | Self::Seeding
                    | Self::Completed
                    | Self::Failed
                    | Self::Cancelled
            ),
            Self::Paused => matches!(next, Self::Queued | Self::Cancelled),
            Self::Verifying => matches!(next, Self::Completed | Self::Failed),
            Self::Seeding => matches!(next, Self::Completed | Self::Paused | Self::Cancelled),
            Self::Failed => matches!(next, Self::Queued | Self::Cancelled),
            Self::Completed | Self::Cancelled => false,
        }
    }

    /// Validates a transition and returns the new state.
    pub fn transition_to(self, next: Self) -> Result<Self, TransitionError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(TransitionError {
                from: self,
                to: next,
            })
        }
    }
}

/// A rejected state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid job state transition from {from:?} to {to:?}")]
pub struct TransitionError {
    pub from: JobState,
    pub to: JobState,
}

/// Where a download comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Url(String),
    Magnet(String),
    TorrentFile(PathBuf),
}

impl Source {
    /// Returns the URL when the source is an HTTP-like URL.
    pub fn url(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url),
            Self::Magnet(_) | Self::TorrentFile(_) => None,
        }
    }
}

/// The protocol implementation that moves bytes for a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Http,
    Torrent,
    Hls,
    Dash,
    YtDlp,
    Plugin,
}

/// Per-job request options.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct JobOptions {
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub segments: Option<u32>,
    pub speed_limit: Option<u64>,
    pub checksum: Option<crate::checksum::Checksum>,
    pub mirrors: Vec<String>,
}

/// Metadata discovered before or while downloading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub backend: BackendKind,
    pub filename: String,
    pub total_bytes: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

/// The last failure recorded on a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobErrorInfo {
    pub message: String,
    pub retryable: bool,
}

/// A download job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    id: JobId,
    source: Source,
    filename: String,
    target_dir: PathBuf,
    state: JobState,
    queue_id: i64,
    position: i64,
    backend: BackendKind,
    total_bytes: Option<u64>,
    downloaded_bytes: u64,
    supports_ranges: bool,
    etag: Option<String>,
    last_modified: Option<String>,
    content_type: Option<String>,
    error: Option<JobErrorInfo>,
    category: Option<String>,
    options: JobOptions,
    created_at: i64,
    updated_at: i64,
}

impl Job {
    /// Creates a new job in the `Queued` state.
    pub fn new(source: Source, target_dir: PathBuf, options: JobOptions) -> Self {
        let now = now_millis();
        Self {
            id: JobId(0),
            source,
            filename: String::new(),
            target_dir,
            state: JobState::Queued,
            queue_id: 1,
            position: 0,
            backend: BackendKind::Http,
            total_bytes: None,
            downloaded_bytes: 0,
            supports_ranges: false,
            etag: None,
            last_modified: None,
            content_type: None,
            error: None,
            category: None,
            options,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn source(&self) -> &Source {
        &self.source
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    pub fn state(&self) -> JobState {
        self.state
    }

    pub fn queue_id(&self) -> i64 {
        self.queue_id
    }

    pub fn position(&self) -> i64 {
        self.position
    }

    pub fn backend(&self) -> BackendKind {
        self.backend
    }

    pub fn total_bytes(&self) -> Option<u64> {
        self.total_bytes
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.downloaded_bytes
    }

    pub fn supports_ranges(&self) -> bool {
        self.supports_ranges
    }

    pub fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn error(&self) -> Option<&JobErrorInfo> {
        self.error.as_ref()
    }

    pub fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }

    pub fn set_category(&mut self, cat: Option<String>) {
        self.category = cat;
        self.updated_at = now_millis();
    }

    pub fn options(&self) -> &JobOptions {
        &self.options
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn updated_at(&self) -> i64 {
        self.updated_at
    }

    /// Final path of the downloaded file.
    pub fn target_path(&self) -> PathBuf {
        self.target_dir.join(self.effective_filename())
    }

    /// Temporary path used while the file is being downloaded.
    pub fn part_path(&self) -> PathBuf {
        self.target_dir
            .join(format!("{}.part", self.effective_filename()))
    }

    /// Applies a validated state transition, leaving the job untouched on error.
    pub fn transition_to(&mut self, next: JobState) -> Result<(), TransitionError> {
        self.state = self.state.transition_to(next)?;
        self.updated_at = now_millis();
        Ok(())
    }

    /// Copies probe metadata into the job.
    pub fn apply_probe(&mut self, probe: ProbeResult) {
        self.backend = probe.backend;
        if self.filename.is_empty() {
            self.filename = probe.filename;
        }
        self.total_bytes = probe.total_bytes;
        self.supports_ranges = probe.supports_ranges;
        self.etag = probe.etag;
        self.last_modified = probe.last_modified;
        self.content_type = probe.content_type;
        self.updated_at = now_millis();
    }

    pub fn set_filename(&mut self, filename: impl Into<String>) {
        self.filename = filename.into();
        self.updated_at = now_millis();
    }

    pub fn set_queue_id(&mut self, queue_id: i64) {
        self.queue_id = queue_id;
        self.updated_at = now_millis();
    }

    pub fn set_backend(&mut self, backend: BackendKind) {
        self.backend = backend;
        self.updated_at = now_millis();
    }

    pub fn set_downloaded_bytes(&mut self, bytes: u64) {
        self.downloaded_bytes = bytes;
        self.updated_at = now_millis();
    }

    pub fn set_error(&mut self, message: impl Into<String>, retryable: bool) {
        self.error = Some(JobErrorInfo {
            message: message.into(),
            retryable,
        });
        self.updated_at = now_millis();
    }

    pub fn clear_error(&mut self) {
        self.error = None;
        self.updated_at = now_millis();
    }

    pub(crate) fn set_id(&mut self, id: JobId) {
        self.id = id;
    }

    pub(crate) fn set_queue_position(&mut self, queue_id: i64, position: i64) {
        self.queue_id = queue_id;
        self.position = position;
    }

    pub(crate) fn record(&self) -> JobRecord {
        JobRecord {
            id: self.id,
            source: self.source.clone(),
            filename: self.filename.clone(),
            target_dir: self.target_dir.clone(),
            state: self.state,
            queue_id: self.queue_id,
            position: self.position,
            backend: self.backend,
            total_bytes: self.total_bytes,
            downloaded_bytes: self.downloaded_bytes,
            supports_ranges: self.supports_ranges,
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
            content_type: self.content_type.clone(),
            error: self.error.clone(),
            category: self.category.clone(),
            options: self.options.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    fn effective_filename(&self) -> &str {
        if self.filename.is_empty() {
            "download"
        } else {
            &self.filename
        }
    }
}

/// Raw field view used by the SQLite store.
pub(crate) struct JobRecord {
    pub id: JobId,
    pub source: Source,
    pub filename: String,
    pub target_dir: PathBuf,
    pub state: JobState,
    pub queue_id: i64,
    pub position: i64,
    pub backend: BackendKind,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub error: Option<JobErrorInfo>,
    pub category: Option<String>,
    pub options: JobOptions,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Job {
    pub(crate) fn from_record(record: JobRecord) -> Self {
        Self {
            id: record.id,
            source: record.source,
            filename: record.filename,
            target_dir: record.target_dir,
            state: record.state,
            queue_id: record.queue_id,
            position: record.position,
            backend: record.backend,
            total_bytes: record.total_bytes,
            downloaded_bytes: record.downloaded_bytes,
            supports_ranges: record.supports_ranges,
            etag: record.etag,
            last_modified: record.last_modified,
            content_type: record.content_type,
            error: record.error,
            category: record.category,
            options: record.options,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}
