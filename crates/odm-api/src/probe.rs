//! Source probing request and response.
use serde::{Deserialize, Serialize};

/// Information about a single file in a torrent source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentFileDto {
    pub index: u32,
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeResponse {
    pub backend: String,
    pub filename: String,
    pub size: Option<u64>,
    pub ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_files: Option<Vec<TorrentFileDto>>,
}
