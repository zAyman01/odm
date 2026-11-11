//! WebSocket events and authentication requests.
use serde::{Deserialize, Serialize};

use crate::download::DownloadItemDto;
use crate::queue::QueueDto;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WsAuthRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WsAuthResponse {
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    JobAdded(DownloadItemDto),
    JobStateChanged {
        id: i64,
        state: String,
    },
    JobProgress {
        id: i64,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed_bps: u64,
    },
    JobRemoved(i64),
    QueueChanged(QueueDto),
}
