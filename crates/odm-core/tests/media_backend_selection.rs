#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::backend::BackendError;
use odm_core::{BackendKind, Engine, JobOptions, Source, Store};
use tempfile::tempdir;

#[test]
fn test_media_backend_selection_by_extension() {
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();
    let dir = tempdir().unwrap();
    let target_dir = dir.path().to_path_buf();

    // 1. .m3u8 -> Hls
    let hls_job = engine
        .add_download(
            Source::Url("https://example.com/stream.m3u8".to_string()),
            target_dir.clone(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(hls_job.backend(), BackendKind::Hls);

    // 2. .mpd -> Dash
    let dash_job = engine
        .add_download(
            Source::Url("https://example.com/manifest.mpd".to_string()),
            target_dir.clone(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(dash_job.backend(), BackendKind::Dash);

    // 3. .zip -> Http
    let http_job = engine
        .add_download(
            Source::Url("https://example.com/file.zip".to_string()),
            target_dir.clone(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(http_job.backend(), BackendKind::Http);

    // 4. .m3u8 with query string -> Hls
    let hls_query_job = engine
        .add_download(
            Source::Url("https://example.com/stream.m3u8?token=abc".to_string()),
            target_dir,
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(hls_query_job.backend(), BackendKind::Hls);
}

#[test]
fn test_job_options_quality_roundtrip() {
    let mut opts = JobOptions::default();
    assert_eq!(opts.quality, None);
    opts.quality = Some("1080p".to_string());

    let json = serde_json::to_string(&opts).unwrap();
    let deserialized: JobOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.quality, Some("1080p".to_string()));

    // Also test deserializing empty object / defaults
    let default_opts: JobOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(default_opts.quality, None);
}

#[test]
fn test_media_backend_errors_retryable() {
    let media_err = BackendError::Media("segment timeout".to_string());
    assert!(media_err.is_retryable());
    assert_eq!(media_err.to_string(), "media error: segment timeout");

    let ytdlp_err = BackendError::YtDlp("rate limit".to_string());
    assert!(ytdlp_err.is_retryable());
    assert_eq!(ytdlp_err.to_string(), "yt-dlp error: rate limit");
}
