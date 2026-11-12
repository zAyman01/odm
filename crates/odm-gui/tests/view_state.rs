//! View-state reducer and filter tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{DownloadItemDto, Event, ProbeResponse};
use odm_gui::messages::GuiEvent;
use odm_gui::state::{Filter, ThemeMode, ViewState};

fn sample_item(id: i64, state: &str, queue: &str) -> DownloadItemDto {
    DownloadItemDto {
        id,
        url: format!("http://127.0.0.1/file{id}.bin"),
        filename: format!("file{id}.bin"),
        state: state.to_string(),
        total_bytes: Some(1024 * 1024),
        downloaded_bytes: 512 * 1024,
        progress_pct: 50.0,
        speed_bps: 128 * 1024,
        queue: queue.to_string(),
        target_dir: "C:\\Downloads".to_string(),
        category: None,
        error: None,
        backend: None,
    }
}

#[test]
fn initial_view_state_defaults() {
    let state = ViewState::default();
    assert_eq!(state.filter, Filter::All);
    assert!(state.downloads.is_empty());
    assert!(state.selected_id.is_none());
    assert!(!state.add_dialog.open);
    assert!(!state.settings_dialog.open);
    assert_eq!(state.theme, ThemeMode::Dark);
}

#[test]
fn filters_partition_downloads_correctly() {
    let mut state = ViewState {
        downloads: vec![
            sample_item(1, "downloading", "default"),
            sample_item(2, "completed", "default"),
            sample_item(3, "paused", "music"),
            sample_item(4, "failed", "default"),
        ],
        ..Default::default()
    };

    state.filter = Filter::All;
    assert_eq!(state.filtered_downloads().len(), 4);

    state.filter = Filter::Active;
    let active = state.filtered_downloads();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, 1);

    state.filter = Filter::Completed;
    let completed = state.filtered_downloads();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, 2);

    state.filter = Filter::Queue("music".to_string());
    let music = state.filtered_downloads();
    assert_eq!(music.len(), 1);
    assert_eq!(music[0].id, 3);
}

#[test]
fn event_reducer_updates_job_progress_and_state() {
    let mut state = ViewState::default();
    state
        .downloads
        .push(sample_item(10, "downloading", "default"));

    // 1. Progress event
    state.apply_event(GuiEvent::EngineEvent(Event::JobProgress {
        id: 10,
        downloaded_bytes: 800 * 1024,
        total_bytes: Some(1024 * 1024),
        speed_bps: 256 * 1024,
    }));
    assert_eq!(state.downloads[0].downloaded_bytes, 800 * 1024);
    assert!((state.downloads[0].progress_pct - 78.1).abs() < 0.2);

    // 2. State change event
    state.apply_event(GuiEvent::EngineEvent(Event::JobStateChanged {
        id: 10,
        state: "completed".to_string(),
    }));
    assert_eq!(state.downloads[0].state, "completed");

    // 3. Job removed event
    state.select_download(Some(10));
    assert_eq!(state.selected_id, Some(10));
    state.apply_event(GuiEvent::EngineEvent(Event::JobRemoved(10)));
    assert!(state.downloads.is_empty());
    assert_eq!(state.selected_id, None);
}

#[test]
fn add_dialog_open_and_probe_lifecycle() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("https://example.com/archive.zip".to_string()));

    assert!(state.add_dialog.open);
    assert_eq!(state.add_dialog.url, "https://example.com/archive.zip");
    assert_eq!(state.add_dialog.filename, "archive.zip");

    state.apply_event(GuiEvent::ProbeResult(Ok(ProbeResponse {
        backend: "http".to_string(),
        filename: "probed_name.zip".to_string(),
        size: Some(5_000_000),
        ranges: true,
        etag: Some("abc".to_string()),
        last_modified: None,
        torrent_files: None,
        qualities: None,
    })));

    assert_eq!(state.add_dialog.filename, "probed_name.zip");
    assert!(state.add_dialog.probed.is_some());

    state.close_add_dialog();
    assert!(!state.add_dialog.open);
}
