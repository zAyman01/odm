use std::io::SeekFrom;
use std::path::Path;
use std::time::{Duration, Instant};

use odm_core::backend::{BackendError, JobContext};
use serde::{Deserialize, Serialize};
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncSeekExt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HlsResumeState {
    pub completed_segments: Vec<u32>,
    pub selected_quality: Option<String>,
    pub total_segments: u32,
    /// Length of the part file when this state was saved. Zero in states saved
    /// by older versions.
    #[serde(default)]
    pub written_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashResumeState {
    pub completed_video_segments: Vec<u32>,
    pub completed_audio_segments: Vec<u32>,
    pub selected_quality: Option<String>,
    pub total_video_segments: u32,
    pub total_audio_segments: u32,
    /// Lengths of the video and audio files when this state was saved. Zero in
    /// states saved by older versions.
    #[serde(default)]
    pub video_bytes: u64,
    #[serde(default)]
    pub audio_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YtDlpResumeState {
    pub format_id: Option<String>,
    pub output_path: String,
}

/// Tells a media download when to save its resume state: every `EVERY_SEGMENTS`
/// segments or `EVERY`, whichever comes first, instead of after each segment.
pub struct SaveThrottle {
    since_save: u32,
    last_save: Instant,
}

impl SaveThrottle {
    const EVERY_SEGMENTS: u32 = 10;
    const EVERY: Duration = Duration::from_secs(2);

    pub fn new() -> Self {
        Self {
            since_save: 0,
            last_save: Instant::now(),
        }
    }

    /// Call once per finished segment. Returns true when the state should be saved now.
    pub fn segment_done(&mut self) -> bool {
        self.since_save += 1;
        if self.since_save < Self::EVERY_SEGMENTS && self.last_save.elapsed() < Self::EVERY {
            return false;
        }
        self.since_save = 0;
        self.last_save = Instant::now();
        true
    }
}

impl Default for SaveThrottle {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes `state` as the job's backend state.
pub(crate) fn save_state<T: Serialize>(ctx: &JobContext, state: &T) -> Result<(), BackendError> {
    let json = serde_json::to_string(state).map_err(|e| BackendError::Media(e.to_string()))?;
    ctx.store.set_backend_state(ctx.job.id(), &json)?;
    Ok(())
}

/// Opens a file that segments are appended to. A fresh download starts empty. A
/// resumed one keeps the first `saved_len` bytes (everything when it is zero):
/// segments written after the last state save are downloaded again, so they must go.
pub(crate) async fn open_for_segments(
    path: &Path,
    resuming: bool,
    saved_len: u64,
) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if !resuming {
        options.truncate(true);
    }
    let mut file = options.open(path).await?;
    if resuming {
        if saved_len > 0 && file.metadata().await?.len() > saved_len {
            file.set_len(saved_len).await?;
        }
        file.seek(SeekFrom::End(0)).await?;
    }
    Ok(file)
}
