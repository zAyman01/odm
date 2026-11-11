//! Named job queues.

/// A named queue with a concurrency limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub id: i64,
    pub name: String,
    pub max_concurrent: u32,
}
