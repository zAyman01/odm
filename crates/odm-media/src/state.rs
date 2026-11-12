use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsResumeState {
    pub completed_segments: Vec<u32>,
    pub selected_quality: Option<String>,
    pub total_segments: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashResumeState {
    pub completed_video_segments: Vec<u32>,
    pub completed_audio_segments: Vec<u32>,
    pub selected_quality: Option<String>,
    pub total_video_segments: u32,
    pub total_audio_segments: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YtDlpResumeState {
    pub format_id: Option<String>,
    pub output_path: String,
}
