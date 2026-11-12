use serde::{Deserialize, Serialize};

/// A selectable quality/format option from a media source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityInfo {
    /// Opaque identifier for this quality (e.g. "720p", "22", bandwidth string).
    pub id: String,
    /// Human-readable resolution (e.g. "1280x720").
    pub resolution: Option<String>,
    /// Bandwidth in bits per second.
    pub bandwidth: Option<u64>,
    /// Codec string (e.g. "avc1.64001f,mp4a.40.2").
    pub codecs: Option<String>,
    /// Human-readable label for display (e.g. "720p - 2.5 Mbps").
    pub label: String,
}
