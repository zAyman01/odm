use serde::{Deserialize, Serialize};

/// Persisted state mapping an odm job to a librqbit torrent handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentResumeState {
    pub torrent_id: usize,
}
