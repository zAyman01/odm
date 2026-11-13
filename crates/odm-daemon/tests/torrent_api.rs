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

#[tokio::test]
async fn seeding_job_releases_queue_slot() {
    use odm_core::JobState;
    use odm_torrent::librqbit::{AddTorrent, AddTorrentOptions, ListenerOptions};

    // A seeder that serves one small file.
    let seed_dir = tempdir().unwrap();
    fs::write(seed_dir.path().join("seed.bin"), vec![0x5A; 32 * 1024]).unwrap();
    let created = create_torrent(
        seed_dir.path(),
        CreateTorrentOptions {
            name: Some("seed.bin"),
            piece_length: Some(16384),
            ..Default::default()
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let seeder = Session::new_with_opts(
        seed_dir.path().to_path_buf(),
        SessionOptions {
            dht: None,
            persistence: None,
            disable_trackers: true,
            listen: Some(ListenerOptions {
                listen_addr: "127.0.0.1:0".parse().unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let seeding = seeder
        .add_torrent(
            AddTorrent::from_bytes(created.as_bytes().unwrap()),
            Some(AddTorrentOptions {
                output_folder: Some(seed_dir.path().to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    seeding.wait_until_completed().await.unwrap();
    let magnet = format!(
        "magnet:?xt=urn:btih:{}&dn=seed.bin&x.pe={}",
        seeding.info_hash().as_string(),
        seeder.listen_addr().unwrap()
    );

    // The daemon under test downloads it with a seed ratio nobody can reach.
    let dir = tempdir().unwrap();
    let engine = Engine::new(Store::open_in_memory().unwrap()).unwrap();
    let leecher = Session::new_with_opts(
        dir.path().join("torrent_data"),
        SessionOptions {
            dht: None,
            persistence: None,
            disable_trackers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let backend = Arc::new(TorrentBackend::new_with_session(leecher, 1.0));
    engine.register_backend(backend.clone());
    let state =
        AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret")).with_torrent(backend);
    state.engine.store().create_queue("solo", 1).unwrap();
    let app = build_router_with_state(state.clone());

    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: magnet,
        target_dir: Some(dir.path().join("out").to_string_lossy().to_string()),
        filename: None,
        queue: Some("solo".to_string()),
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
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let id = serde_json::from_slice::<AddDownloadResponse>(&bytes)
        .unwrap()
        .id;

    let semaphore = state.get_semaphore("solo", 1);
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let seeding = state
                .engine
                .store()
                .get_job(odm_core::JobId(id))
                .is_ok_and(|job| job.state() == JobState::Seeding);
            if seeding && semaphore.available_permits() == 1 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("a seeding job must give its queue slot back");
}
