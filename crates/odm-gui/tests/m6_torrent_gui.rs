//! GUI tests for M6 torrent support: file selection in add dialog, seed ratio override,
//! sidebar Torrents filter, and seeding state display.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{
    DownloadDetailDto, DownloadItemDto, Event, JobOptionsDto, ProbeResponse, TorrentFileDto,
};
use odm_gui::messages::GuiEvent;
use odm_gui::state::{Filter, ViewState};

fn sample_item(id: i64, state: &str, backend: Option<&str>) -> DownloadItemDto {
    DownloadItemDto {
        id,
        url: format!("https://example.com/file{id}.bin"),
        filename: format!("file{id}.bin"),
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
fn add_dialog_torrent_files_probed_and_selected_by_default() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("magnet:?xt=urn:btih:0123".to_string()));

    let probe_resp = ProbeResponse {
        backend: "torrent".to_string(),
        filename: "MyTorrent".to_string(),
        size: Some(1024 * 1024),
        ranges: true,
        etag: None,
        last_modified: None,
        torrent_files: Some(vec![
            TorrentFileDto {
                index: 0,
                path: "file1.txt".to_string(),
                size: 500,
            },
            TorrentFileDto {
                index: 1,
                path: "file2.txt".to_string(),
                size: 524,
            },
        ]),
    };

    state.apply_event(GuiEvent::ProbeResult(Ok(probe_resp)));

    assert_eq!(state.add_dialog.torrent_files.len(), 2);
    assert!(state.add_dialog.selected_file_indices.contains(&0));
    assert!(state.add_dialog.selected_file_indices.contains(&1));

    let requests = state.build_add_download_requests();
    assert_eq!(requests.len(), 1);
    let options = requests[0].options.as_ref().unwrap();
    assert_eq!(options.selected_files.as_deref(), Some(&[0, 1][..]));
}

#[test]
fn add_dialog_toggle_file_selection_and_seed_ratio() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("magnet:?xt=urn:btih:0123".to_string()));

    let probe_resp = ProbeResponse {
        backend: "torrent".to_string(),
        filename: "MyTorrent".to_string(),
        size: Some(1024 * 1024),
        ranges: true,
        etag: None,
        last_modified: None,
        torrent_files: Some(vec![
            TorrentFileDto {
                index: 0,
                path: "file1.txt".to_string(),
                size: 500,
            },
            TorrentFileDto {
                index: 1,
                path: "file2.txt".to_string(),
                size: 524,
            },
        ]),
    };

    state.apply_event(GuiEvent::ProbeResult(Ok(probe_resp)));

    // Deselect file index 1 and set seed ratio
    state.add_dialog.selected_file_indices.remove(&1);
    state.add_dialog.seed_ratio = Some(1.5);

    let requests = state.build_add_download_requests();
    assert_eq!(requests.len(), 1);
    let options = requests[0].options.as_ref().unwrap();
    assert_eq!(options.selected_files.as_deref(), Some(&[0][..]));
    assert_eq!(options.seed_ratio, Some(1.5));
}

#[test]
fn sidebar_torrents_filter() {
    let state = ViewState {
        downloads: vec![
            sample_item(1, "completed", Some("http")),
            sample_item(2, "seeding", Some("torrent")),
            sample_item(3, "downloading", Some("torrent")),
            sample_item(4, "downloading", None),
        ],
        filter: Filter::Torrents,
        ..Default::default()
    };

    let filtered = state.filtered_downloads();
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].id, 2);
    assert_eq!(filtered[1].id, 3);
}

#[test]
fn seeding_state_and_progress_display() {
    let mut state = ViewState {
        downloads: vec![sample_item(2, "downloading", Some("torrent"))],
        ..Default::default()
    };

    state.apply_event(GuiEvent::EngineEvent(Event::JobStateChanged {
        id: 2,
        state: "seeding".to_string(),
    }));

    assert_eq!(state.downloads[0].state, "seeding");

    state.select_download(Some(2));
    let detail = DownloadDetailDto {
        item: state.downloads[0].clone(),
        segments: vec![],
        options: JobOptionsDto {
            seed_ratio: Some(2.0),
            ..Default::default()
        },
        backend: Some("torrent".to_string()),
    };
    state.apply_event(GuiEvent::DownloadDetail(Box::new(detail)));

    assert_eq!(
        state.selected_detail.as_ref().unwrap().item.state,
        "seeding"
    );
    assert_eq!(
        state.selected_detail.as_ref().unwrap().options.seed_ratio,
        Some(2.0)
    );
}
