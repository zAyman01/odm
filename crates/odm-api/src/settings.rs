//! Application settings models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    pub default_segments: u32,
    pub global_speed_limit: u64,
    pub default_retry_count: u32,
    #[serde(default)]
    pub seed_ratio: f64,
    #[serde(default)]
    pub torrent_listen_port: u16,
    #[serde(default)]
    pub ffmpeg_path: String,
    #[serde(default)]
    pub ytdlp_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdateSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_speed_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_retry_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torrent_listen_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ytdlp_path: Option<String>,
}
