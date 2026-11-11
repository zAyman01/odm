//! Download job requests, responses and DTOs.
use serde::{Deserialize, Serialize};

use crate::checksum::ChecksumDto;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobOptionsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<(String, String)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<ChecksumDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirrors: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddDownloadRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<JobOptionsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddDownloadResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadItemDto {
    pub id: i64,
    pub url: String,
    pub filename: String,
    pub state: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub progress_pct: f32,
    pub speed_bps: u64,
    pub queue: String,
    pub target_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDto {
    pub index: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadDetailDto {
    pub item: DownloadItemDto,
    pub segments: Vec<SegmentDto>,
    pub options: JobOptionsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListDownloadsResponse {
    pub downloads: Vec<DownloadItemDto>,
}
