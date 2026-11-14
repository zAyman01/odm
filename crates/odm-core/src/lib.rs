//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
#![forbid(unsafe_code)]

pub mod backend;
pub mod batch;
pub mod category;
pub mod checksum;
pub mod engine;
pub mod event;
pub mod filename;
pub mod grabber;
pub mod http;
pub mod job;
pub mod limiter;
pub mod metalink;
pub mod queue;
pub mod schedule;
pub mod segment;
pub mod store;

mod util;

pub use backend::{
    Backend, BackendError, ControlCommand, ControlHandle, ControlToken, JobContext, ProgressSink,
    ProgressUpdate, RunOutcome,
};
pub use batch::{BatchError, expand_batch_url};
pub use category::Category;
pub use checksum::{Checksum, ChecksumAlgorithm, ChecksumError, verify_file_checksum};
pub use engine::{AddOverrides, Engine, EngineError};
pub use event::Event;
pub use filename::{sanitize_filename, stream_filename};
pub use grabber::{GrabResult, GrabbedFile, GrabberConfig, GrabberError, RobotsTxt, SiteGrabber};
pub use http::{HttpBackend, apply_options};
pub use job::{
    BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobState, ProbeResult, Source,
    TransitionError,
};
pub use limiter::{Limiter, TokenBucket};
pub use metalink::{MetalinkError, MetalinkSpec, parse_metalink};
pub use queue::Queue;
pub use schedule::{Clock, QueueSchedule, SystemClock};
pub use segment::Segment;
pub use store::{Store, StoreError};
