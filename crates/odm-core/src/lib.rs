//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
#![forbid(unsafe_code)]

pub mod backend;
pub mod http;
pub mod job;
pub mod store;

mod util;

pub use backend::{
    Backend, BackendError, ControlCommand, ControlHandle, ControlToken, JobContext, ProgressSink,
    ProgressUpdate, RunOutcome,
};
pub use http::HttpBackend;
pub use job::{
    BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobState, ProbeResult, Source,
    TransitionError,
};
pub use store::{Store, StoreError};
