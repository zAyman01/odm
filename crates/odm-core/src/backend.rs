//! Backend contract shared by every protocol implementation.

use async_trait::async_trait;
use tokio::sync::{mpsc, watch};

use crate::job::{BackendKind, Job, JobOptions, JobState, ProbeResult, Source};
use crate::limiter::Limiter;
use crate::store::{Store, StoreError};

/// A protocol implementation that probes sources and moves bytes.
#[async_trait]
pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError>;

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError>;
}

/// How a backend run finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Paused,
    Cancelled,
}

/// A command sent to a running backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlCommand {
    #[default]
    Run,
    Pause,
    Cancel,
}

/// The sending half of a job's control channel.
#[derive(Clone)]
pub struct ControlHandle {
    sender: watch::Sender<ControlCommand>,
}

impl ControlHandle {
    pub fn pause(&self) {
        self.sender.send_replace(ControlCommand::Pause);
    }

    pub fn cancel(&self) {
        self.sender.send_replace(ControlCommand::Cancel);
    }

    pub fn resume(&self) {
        self.sender.send_replace(ControlCommand::Run);
    }
}

/// The receiving half of a job's control channel.
#[derive(Clone)]
pub struct ControlToken {
    receiver: watch::Receiver<ControlCommand>,
}

impl ControlToken {
    pub fn new() -> (ControlHandle, Self) {
        let (sender, receiver) = watch::channel(ControlCommand::Run);
        (ControlHandle { sender }, Self { receiver })
    }

    pub fn command(&self) -> ControlCommand {
        *self.receiver.borrow()
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.command(), ControlCommand::Pause)
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self.command(), ControlCommand::Cancel)
    }
}

/// A progress report from a running backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressUpdate {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    /// A state the backend moved the job into itself, such as `Seeding`.
    pub state: Option<JobState>,
}

/// The sending half of a job's progress channel.
#[derive(Clone)]
pub struct ProgressSink {
    sender: mpsc::UnboundedSender<ProgressUpdate>,
}

impl ProgressSink {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<ProgressUpdate>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    /// Reports progress without blocking; a closed receiver is ignored.
    pub fn report(&self, downloaded_bytes: u64, total_bytes: Option<u64>) {
        let _ = self.sender.send(ProgressUpdate {
            downloaded_bytes,
            total_bytes,
            state: None,
        });
    }

    /// Tells the engine the backend moved the job into `state`, so it can
    /// announce the change. The backend has already stored the new state.
    pub fn report_state(&self, state: JobState, downloaded_bytes: u64, total_bytes: Option<u64>) {
        let _ = self.sender.send(ProgressUpdate {
            downloaded_bytes,
            total_bytes,
            state: Some(state),
        });
    }
}

/// Everything a backend needs to run one job.
pub struct JobContext {
    pub job: Job,
    pub control: ControlToken,
    pub progress: ProgressSink,
    pub store: Store,
    pub limiter: Limiter,
}

/// Errors returned by backends.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("server ignored the requested byte range")]
    RangeIgnored,
    #[error("worker task failed: {0}")]
    Task(String),
    #[error("torrent error: {0}")]
    Torrent(String),
    #[error("media error: {0}")]
    Media(String),
    #[error("yt-dlp error: {0}")]
    YtDlp(String),
    #[error("plugin error: {0}")]
    Plugin(String),
    #[error("source is not supported by this backend")]
    Unsupported,
    #[error("unexpected http status: {0}")]
    Status(reqwest::StatusCode),
    #[error("invalid request options: {0}")]
    InvalidOptions(String),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
}

impl BackendError {
    /// Returns whether retrying the same request may succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(_) | Self::Torrent(_) | Self::Media(_) | Self::YtDlp(_) => true,
            Self::Status(status) => {
                matches!(status.as_u16(), 408 | 429) || status.is_server_error()
            }
            Self::Io(_)
            | Self::Store(_)
            | Self::RangeIgnored
            | Self::Task(_)
            | Self::Unsupported
            | Self::InvalidOptions(_)
            | Self::InvalidUrl(_)
            | Self::Plugin(_) => false,
        }
    }
}
