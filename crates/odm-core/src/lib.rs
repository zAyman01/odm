//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
#![forbid(unsafe_code)]

pub mod backend;
pub mod batch;
pub mod checksum;
pub mod engine;
pub mod event;
pub mod http;
pub mod job;
pub mod limiter;
pub mod metalink;
pub mod queue;
pub mod segment;
pub mod store;

mod util;

pub use backend::{
    Backend, BackendError, ControlCommand, ControlHandle, ControlToken, JobContext, ProgressSink,
    ProgressUpdate, RunOutcome,
};
pub use batch::{BatchError, expand_batch_url};
pub use checksum::{Checksum, ChecksumAlgorithm, ChecksumError, verify_file_checksum};
pub use engine::{Engine, EngineError};
pub use event::Event;
pub use http::HttpBackend;
pub use job::{
    BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobState, ProbeResult, Source,
    TransitionError,
};
pub use limiter::{Limiter, TokenBucket};
pub use metalink::{MetalinkError, MetalinkSpec, parse_metalink};
pub use queue::Queue;
pub use segment::Segment;
pub use store::{Store, StoreError};
