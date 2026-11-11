//! Queue management models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueDto {
    pub id: i64,
    pub name: String,
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateQueueRequest {
    pub name: String,
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateQueueRequest {
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListQueuesResponse {
    pub queues: Vec<QueueDto>,
}
