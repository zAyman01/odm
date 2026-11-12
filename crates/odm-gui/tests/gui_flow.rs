//! End-to-end GUI flow test: simulates complete user interaction flow through the ViewState.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{DownloadItemDto, Event, ProbeResponse};
use odm_gui::messages::GuiEvent;
use odm_gui::state::{Filter, ViewState};

#[test]
fn full_gui_lifecycle_flow() {
    let mut state = ViewState::default();

    // 1. User copies URL, clipboard emits URL captured event
    state.apply_event(GuiEvent::UrlCaptured(
        "http://127.0.0.1:8080/sample.tar.gz".to_string(),
    ));
    assert!(state.add_dialog.open);
    assert_eq!(state.add_dialog.url, "http://127.0.0.1:8080/sample.tar.gz");

    // 2. Probe returns metadata
    state.apply_event(GuiEvent::ProbeResult(Ok(ProbeResponse {
        backend: "http".to_string(),
        filename: "sample.tar.gz".to_string(),
        size: Some(10_000_000),
        ranges: true,
        etag: Some("w/123".to_string()),
        last_modified: None,
        torrent_files: None,
    })));
    assert_eq!(state.add_dialog.filename, "sample.tar.gz");

    // 3. User submits download
    state.close_add_dialog();

    // 4. Daemon confirms download added
    let item = DownloadItemDto {
        id: 42,
        url: "http://127.0.0.1:8080/sample.tar.gz".to_string(),
        filename: "sample.tar.gz".to_string(),
        state: "downloading".to_string(),
        total_bytes: Some(10_000_000),
        downloaded_bytes: 0,
        progress_pct: 0.0,
        speed_bps: 0,
        queue: "default".to_string(),
        target_dir: "C:\\Downloads".to_string(),
        category: None,
        error: None,
        backend: None,
    };
    state.apply_event(GuiEvent::EngineEvent(Event::JobAdded(item)));
    assert_eq!(state.downloads.len(), 1);
    assert_eq!(state.downloads[0].id, 42);

    // 5. Select item and update progress
    state.select_download(Some(42));
    assert_eq!(state.selected_id, Some(42));

    state.apply_event(GuiEvent::EngineEvent(Event::JobProgress {
        id: 42,
        downloaded_bytes: 5_000_000,
        total_bytes: Some(10_000_000),
        speed_bps: 1_000_000,
    }));
    assert_eq!(state.downloads[0].downloaded_bytes, 5_000_000);
    assert_eq!(state.downloads[0].progress_pct, 50.0);

    // 6. Complete job
    state.apply_event(GuiEvent::EngineEvent(Event::JobStateChanged {
        id: 42,
        state: "completed".to_string(),
    }));
    assert_eq!(state.downloads[0].state, "completed");

    // 7. Verify filtering
    state.filter = Filter::Active;
    assert!(state.filtered_downloads().is_empty());
    state.filter = Filter::Completed;
    assert_eq!(state.filtered_downloads().len(), 1);

    // 8. Delete job
    state.apply_event(GuiEvent::EngineEvent(Event::JobRemoved(42)));
    assert!(state.downloads.is_empty());
    assert_eq!(state.selected_id, None);
}
