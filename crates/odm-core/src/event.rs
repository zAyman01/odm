//! Broadcast event bus domain events.
use crate::job::{JobId, JobState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    JobAdded(JobId),
    JobStateChanged {
        id: JobId,
        state: JobState,
    },
    JobProgress {
        id: JobId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed_bps: u64,
    },
    JobRemoved(JobId),
    QueueChanged(String),
}
