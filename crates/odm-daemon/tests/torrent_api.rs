#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, ProbeRequest, ProbeResponse,
    SettingsDto, UpdateSettingsRequest,
};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;
use odm_torrent::TorrentBackend;
use odm_torrent::librqbit::spawn_utils::BlockingSpawner;
use odm_torrent::librqbit::{CreateTorrentOptions, Session, SessionOptions, create_torrent};
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn torrent_api_lifecycle() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();

    let torrent_dir = dir.path().join("torrent_data");
    fs::create_dir_all(&torrent_dir).unwrap();
    let torrent_session = Session::new_with_opts(
        torrent_dir,
        SessionOptions {
            dht: None,
            persistence: None,
            disable_trackers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let torrent_backend = Arc::new(TorrentBackend::new_with_session(torrent_session, 0.0));
    engine.register_backend(torrent_backend.clone());

    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config).with_torrent(torrent_backend);
    let app = build_router_with_state(state);

    // 1. Create a test .torrent file for fast probing
    let files_dir = dir.path().join("files");
    fs::create_dir_all(&files_dir).unwrap();
    let sample_file = files_dir.join("sample_torrent_file.bin");
    fs::write(&sample_file, vec![0xAB; 32 * 1024]).unwrap();
    let spawner = BlockingSpawner::new(1);
    let create_result = create_torrent(
        &files_dir,
        CreateTorrentOptions {
            name: Some("sample_torrent_file.bin"),
            piece_length: Some(16384),
            ..Default::default()
        },
        &spawner,
    )
    .await
    .unwrap();
    let torrent_path = dir.path().join("test.torrent");
    fs::write(&torrent_path, create_result.as_bytes().unwrap()).unwrap();

    // 2. Probe .torrent file path
    let probe_body = serde_json::to_vec(&ProbeRequest {
        url: torrent_path.to_string_lossy().to_string(),
        user_agent: None,
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/probe")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(probe_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let probe: ProbeResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(probe.backend, "torrent");
    assert_eq!(probe.filename, "sample_torrent_file.bin");
    assert_eq!(probe.size, Some(32 * 1024));
    assert!(probe.torrent_files.is_some());

    // 3. Add magnet download (creates torrent job without needing live peers)
    let magnet_uri =
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=my_magnet_file.bin";
    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: magnet_uri.to_string(),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: Some("my_magnet_file.bin".to_string()),
        queue: Some("default".to_string()),
        category: None,
        options: None,
        backend: None,
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(add_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();

    // 4. GET /api/v1/downloads/{id}
    let req = axum::http::Request::builder()
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(detail.backend.as_deref(), Some("torrent"));

    // 5. PATCH /api/v1/settings with seed_ratio and torrent_listen_port
    let patch_body = serde_json::to_vec(&UpdateSettingsRequest {
        seed_ratio: Some(1.5),
        torrent_listen_port: Some(6881),
        ..Default::default()
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("PATCH")
        .uri("/api/v1/settings")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(patch_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
    let settings: SettingsDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(settings.seed_ratio, 1.5);
    assert_eq!(settings.torrent_listen_port, 6881);
}
