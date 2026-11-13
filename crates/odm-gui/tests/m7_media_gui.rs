#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{DownloadItemDto, ProbeResponse, QualityDto, SettingsDto};
use odm_gui::messages::{GuiEvent, UserAction};
use odm_gui::state::{ViewState, backend_display};

fn sample_item(id: i64, state: &str, backend: Option<&str>) -> DownloadItemDto {
    DownloadItemDto {
        id,
        url: format!("https://example.com/stream{id}.m3u8"),
        filename: format!("stream{id}.mp4"),
        state: state.to_string(),
        total_bytes: Some(1_000_000),
        downloaded_bytes: 500_000,
        progress_pct: 50.0,
        speed_bps: 100_000,
        queue: "default".to_string(),
        target_dir: "C:\\Downloads".to_string(),
        category: None,
        error: None,
        backend: backend.map(|s| s.to_string()),
    }
}

#[test]
fn add_dialog_qualities_probed_and_first_selected_by_default() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("https://example.com/video.m3u8".to_string()));

    let probe_resp = ProbeResponse {
        backend: "hls".to_string(),
        filename: "video.mp4".to_string(),
        size: Some(1024 * 1024),
        ranges: true,
        etag: None,
        last_modified: None,
        torrent_files: None,
        qualities: Some(vec![
            QualityDto {
                id: "1080p".to_string(),
                resolution: Some("1920x1080".to_string()),
                bandwidth: Some(5_000_000),
                codecs: Some("avc1.64002a".to_string()),
                label: "1080p (5.0 Mbps)".to_string(),
            },
            QualityDto {
                id: "720p".to_string(),
                resolution: Some("1280x720".to_string()),
                bandwidth: Some(2_500_000),
                codecs: Some("avc1.4d401f".to_string()),
                label: "720p (2.5 Mbps)".to_string(),
            },
        ]),
    };

    state.apply_event(GuiEvent::ProbeResult(Ok(probe_resp)));

    assert_eq!(state.add_dialog.qualities.len(), 2);
    assert_eq!(state.add_dialog.selected_quality.as_deref(), Some("1080p"));

    let req = state.build_add_request().unwrap();
    let options = req.options.as_ref().unwrap();
    assert_eq!(options.quality.as_deref(), Some("1080p"));
}

#[test]
fn add_dialog_select_quality_updates_selected_quality_and_request() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("https://example.com/video.mpd".to_string()));

    let probe_resp = ProbeResponse {
        backend: "dash".to_string(),
        filename: "video.mp4".to_string(),
        size: None,
        ranges: true,
        etag: None,
        last_modified: None,
        torrent_files: None,
        qualities: Some(vec![
            QualityDto {
                id: "1080p".to_string(),
                resolution: Some("1920x1080".to_string()),
                bandwidth: Some(5_000_000),
                codecs: None,
                label: "1080p".to_string(),
            },
            QualityDto {
                id: "720p".to_string(),
                resolution: Some("1280x720".to_string()),
                bandwidth: Some(2_500_000),
                codecs: None,
                label: "720p".to_string(),
            },
        ]),
    };

    state.apply_event(GuiEvent::ProbeResult(Ok(probe_resp)));
    assert_eq!(state.add_dialog.selected_quality.as_deref(), Some("1080p"));

    // Select 720p via UserAction
    state.handle_action(&UserAction::SelectQuality("720p".to_string()));
    assert_eq!(state.add_dialog.selected_quality.as_deref(), Some("720p"));

    let req = state.build_add_request().unwrap();
    let options = req.options.as_ref().unwrap();
    assert_eq!(options.quality.as_deref(), Some("720p"));

    // Also check build_add_download_requests
    let reqs = state.build_add_download_requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        reqs[0].options.as_ref().unwrap().quality.as_deref(),
        Some("720p")
    );
}

#[test]
fn settings_dialog_ffmpeg_and_ytdlp_paths() {
    let mut state = ViewState::default();
    state.open_settings_dialog();

    // Check loading settings
    let settings = SettingsDto {
        download_dir: Some("/downloads".to_string()),
        default_segments: 8,
        global_speed_limit: 0,
        default_retry_count: 3,
        seed_ratio: 0.0,
        torrent_listen_port: 6881,
        ffmpeg_path: "/opt/ffmpeg/bin/ffmpeg".to_string(),
        ytdlp_path: "/opt/yt-dlp".to_string(),
        allowed_hosts: String::new(),
    };

    state.settings_dialog.load_settings(&settings);
    assert_eq!(state.settings_dialog.ffmpeg_path, "/opt/ffmpeg/bin/ffmpeg");
    assert_eq!(state.settings_dialog.ytdlp_path, "/opt/yt-dlp");

    let req = state.settings_dialog.build_update_settings_request();
    assert_eq!(req.ffmpeg_path.as_deref(), Some("/opt/ffmpeg/bin/ffmpeg"));
    assert_eq!(req.ytdlp_path.as_deref(), Some("/opt/yt-dlp"));

    // Also verify ViewState convenience method
    let req2 = state.build_update_settings_request();
    assert_eq!(req2.ffmpeg_path.as_deref(), Some("/opt/ffmpeg/bin/ffmpeg"));
    assert_eq!(req2.ytdlp_path.as_deref(), Some("/opt/yt-dlp"));
}

#[test]
fn backend_display_shows_media_backends() {
    assert_eq!(backend_display(Some("hls")), "HLS");
    assert_eq!(backend_display(Some("dash")), "DASH");
    assert_eq!(backend_display(Some("yt-dlp")), "yt-dlp");
    assert_eq!(backend_display(Some("torrent")), "Torrent");
    assert_eq!(backend_display(Some("http")), "HTTP");
    assert_eq!(backend_display(None), "HTTP");

    let item_hls = sample_item(1, "downloading", Some("hls"));
    assert_eq!(backend_display(item_hls.backend.as_deref()), "HLS");

    let item_dash = sample_item(2, "downloading", Some("dash"));
    assert_eq!(backend_display(item_dash.backend.as_deref()), "DASH");

    let item_ytdlp = sample_item(3, "downloading", Some("yt-dlp"));
    assert_eq!(backend_display(item_ytdlp.backend.as_deref()), "yt-dlp");
}
