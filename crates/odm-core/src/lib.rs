//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
#![forbid(unsafe_code)]

pub mod job;

mod util;

pub use job::{
    BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobState, ProbeResult, Source,
    TransitionError,
};
