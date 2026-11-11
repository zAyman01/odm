//! GUI M5 integration tests: batch URL expansion preview, checksum/mirrors state, category sync and filtering.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{CategoryDto, ChecksumAlgorithm, DownloadItemDto};
use odm_gui::messages::GuiEvent;
use odm_gui::state::{Filter, ViewState};

fn sample_item(id: i64, state: &str, category: Option<&str>) -> DownloadItemDto {
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
        category: category.map(|s| s.to_string()),
        error: None,
    }
}

#[test]
fn add_dialog_batch_url_expansion_preview() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("http://example.com/file[1-3].bin".to_string()));

    assert_eq!(state.add_dialog.batch_preview.len(), 3);
    assert_eq!(
        state.add_dialog.batch_preview[0],
        "http://example.com/file1.bin"
    );
    assert_eq!(
        state.add_dialog.batch_preview[1],
        "http://example.com/file2.bin"
    );
    assert_eq!(
        state.add_dialog.batch_preview[2],
        "http://example.com/file3.bin"
    );
}

#[test]
fn add_dialog_checksum_and_mirrors_fields() {
    let mut state = ViewState::default();
    state.open_add_dialog(Some("http://example.com/archive.zip".to_string()));
    state.add_dialog.checksum_algo = "sha256".to_string();
    state.add_dialog.checksum_hash =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string();
    state.add_dialog.mirrors =
        "http://mirror1.com/archive.zip\nhttp://mirror2.com/archive.zip".to_string();

    let requests = state.build_add_download_requests();
    assert_eq!(requests.len(), 1);
    let req = &requests[0];
    assert_eq!(req.url, "http://example.com/archive.zip");

    let options = req.options.as_ref().unwrap();
    let cs = options.checksum.as_ref().unwrap();
    assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
    assert_eq!(
        cs.digest,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    let mirrors = options.mirrors.as_ref().unwrap();
    assert_eq!(mirrors.len(), 2);
    assert_eq!(mirrors[0], "http://mirror1.com/archive.zip");
    assert_eq!(mirrors[1], "http://mirror2.com/archive.zip");
}

#[test]
fn category_sync_and_filtering() {
    let mut state = ViewState::default();
    state.apply_event(GuiEvent::CategoriesList(vec![
        CategoryDto {
            name: "Compressed".to_string(),
            directory: "/downloads/compressed".to_string(),
            extensions: vec!["zip".to_string(), "rar".to_string()],
        },
        CategoryDto {
            name: "Music".to_string(),
            directory: "/downloads/music".to_string(),
            extensions: vec!["mp3".to_string()],
        },
    ]));

    assert_eq!(state.categories.len(), 2);

    state.downloads = vec![
        sample_item(1, "downloading", Some("Compressed")),
        sample_item(2, "completed", Some("Music")),
        sample_item(3, "downloading", Some("Compressed")),
    ];

    state.filter = Filter::Category("Compressed".to_string());
    let filtered = state.filtered_downloads();
    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .all(|d| d.category.as_deref() == Some("Compressed"))
    );

    state.filter = Filter::Category("Music".to_string());
    let music_filtered = state.filtered_downloads();
    assert_eq!(music_filtered.len(), 1);
    assert_eq!(music_filtered[0].id, 2);
}
