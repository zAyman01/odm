//! Site grabber API request and response types.

use serde::{Deserialize, Serialize};

/// Request payload for site grabber operations (`POST /api/v1/grabber`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrabberRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub respect_robots: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pages: Option<usize>,
}

/// A file found during a site grab / crawl.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrabbedFileDto {
    pub url: String,
    pub filename: String,
    pub found_on: String,
    pub depth: u32,
}

/// Response payload from site grabber operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrabberResponse {
    pub files: Vec<GrabbedFileDto>,
    pub pages_crawled: usize,
}
