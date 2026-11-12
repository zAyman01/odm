//! HLS, DASH and yt-dlp backends for odm.
#![forbid(unsafe_code)]

pub mod dash;
pub mod hls;
pub mod mux;
pub mod quality;
pub mod state;

pub use dash::DashBackend;
pub use hls::HlsBackend;
