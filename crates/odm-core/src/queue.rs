//! Named job queues.

use crate::schedule::QueueSchedule;

/// A named queue with a concurrency limit and optional schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub id: i64,
    pub name: String,
    pub max_concurrent: u32,
    pub schedule: Option<QueueSchedule>,
}
