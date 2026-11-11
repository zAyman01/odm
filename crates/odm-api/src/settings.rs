//! Application settings models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    pub default_segments: u32,
    pub global_speed_limit: u64,
    pub default_retry_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_speed_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_retry_count: Option<u32>,
}
