#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerOptions, Session, SessionOptions,
    create_torrent,
};
use odm_core::backend::{Backend, ControlToken, JobContext, ProgressSink, RunOutcome};
use odm_core::job::{BackendKind, Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_torrent::TorrentBackend;
use tempfile::TempDir;

struct TestHarness {
    _source_dir: TempDir,
    _seeder_session: std::sync::Arc<Session>,
    _seeder_addr: SocketAddr,
    magnet_uri: String,
    test_bytes: Vec<u8>,
}

async fn setup_seeder() -> TestHarness {
    let source_dir = TempDir::new().unwrap();
    let file_path = source_dir.path().join("test_file.bin");
    let test_bytes: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
    fs::write(&file_path, &test_bytes).unwrap();

    let spawner = BlockingSpawner::new(1);
    let create_result = create_torrent(
        source_dir.path(),
        CreateTorrentOptions {
            name: Some("test_file.bin"),
            piece_length: Some(16384),
            ..Default::default()
        },
        &spawner,
    )
    .await
    .unwrap();

    let torrent_bytes = create_result.as_bytes().unwrap();

    let seeder_session = Session::new_with_opts(
        source_dir.path().to_path_buf(),
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

    let seeder_handle = seeder_session
        .add_torrent(
            AddTorrent::from_bytes(torrent_bytes),
            Some(AddTorrentOptions {
                paused: false,
                output_folder: Some(source_dir.path().to_string_lossy().to_string()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();

    seeder_handle.wait_until_completed().await.unwrap();

    let seeder_addr = seeder_session.listen_addr().unwrap();
    let info_hash = seeder_handle.info_hash();
    let magnet_uri = format!(
        "magnet:?xt=urn:btih:{}&dn=test_file.bin&x.pe={}",
        info_hash.as_string(),
        seeder_addr
    );

    TestHarness {
        _source_dir: source_dir,
        _seeder_session: seeder_session,
        _seeder_addr: seeder_addr,
        magnet_uri,
        test_bytes,
    }
}

#[tokio::test]
async fn probe_magnet_returns_metadata() {
    let harness = setup_seeder().await;
    let leecher_dir = TempDir::new().unwrap();
    let leecher_session = Session::new_with_opts(
        leecher_dir.path().to_path_buf(),
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

    let backend = TorrentBackend::new_with_session(leecher_session, 0.0);
    let probe = backend
        .probe(
            &Source::Magnet(harness.magnet_uri.clone()),
            &JobOptions::default(),
        )
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Torrent);
    assert_eq!(probe.filename, "test_file.bin");
    assert_eq!(probe.total_bytes, Some(harness.test_bytes.len() as u64));
    assert!(probe.supports_ranges);
}

#[tokio::test]
async fn probe_torrent_file_returns_metadata_and_files() {
    let source_dir = TempDir::new().unwrap();
    let file_path = source_dir.path().join("file1.bin");
    let test_bytes = vec![42u8; 1024];
    fs::write(&file_path, &test_bytes).unwrap();

    let spawner = BlockingSpawner::new(1);
    let create_result = create_torrent(
        source_dir.path(),
        CreateTorrentOptions {
            name: Some("file1.bin"),
            piece_length: Some(16384),
            ..Default::default()
        },
        &spawner,
    )
    .await
    .unwrap();

    let torrent_file_path = source_dir.path().join("test.torrent");
    fs::write(&torrent_file_path, create_result.as_bytes().unwrap()).unwrap();

    let leecher_dir = TempDir::new().unwrap();
    let leecher_session = Session::new_with_opts(
        leecher_dir.path().to_path_buf(),
        SessionOptions {
            dht: None,
            persistence: None,
            disable_trackers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let backend = TorrentBackend::new_with_session(leecher_session, 0.0);
    let source = Source::TorrentFile(torrent_file_path);
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Torrent);
    assert_eq!(probe.filename, "file1.bin");
    assert_eq!(probe.total_bytes, Some(1024));

    let files = backend.probe_torrent_files(&source).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].size, 1024);
}

#[tokio::test]
async fn run_download_completes_and_matches_content() {
    let harness = setup_seeder().await;
    let leecher_dir = TempDir::new().unwrap();
    let leecher_session = Session::new_with_opts(
        leecher_dir.path().to_path_buf(),
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

    let backend = TorrentBackend::new_with_session(leecher_session, 0.0);
    let store = Store::open_in_memory().unwrap();
    let mut job = Job::new(
        Source::Magnet(harness.magnet_uri.clone()),
        leecher_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    job.set_backend(BackendKind::Torrent);
    job.set_filename("test_file.bin");
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control_token) = ControlToken::new();
    let (progress_sink, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job,
        control: control_token,
        progress: progress_sink,
        store,
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    // Verify progress updates arrived
    let mut last_progress = None;
    while let Ok(update) = progress_rx.try_recv() {
        last_progress = Some(update);
    }
    assert!(last_progress.is_some());

    // Verify downloaded file content
    let downloaded_file = leecher_dir.path().join("test_file.bin");
    assert!(downloaded_file.exists());
    let downloaded_bytes = fs::read(downloaded_file).unwrap();
    assert_eq!(downloaded_bytes, harness.test_bytes);
}

#[tokio::test]
async fn run_download_can_be_paused() {
    let harness = setup_seeder().await;
    let leecher_dir = TempDir::new().unwrap();
    let leecher_session = Session::new_with_opts(
        leecher_dir.path().to_path_buf(),
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

    let backend = TorrentBackend::new_with_session(leecher_session, 0.0);
    let store = Store::open_in_memory().unwrap();
    let mut job = Job::new(
        Source::Magnet(harness.magnet_uri.clone()),
        leecher_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    job.set_backend(BackendKind::Torrent);
    job.set_filename("test_file.bin");
    store.insert_job(&mut job).unwrap();

    let (control_handle, control_token) = ControlToken::new();
    let (progress_sink, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job,
        control: control_token,
        progress: progress_sink,
        store,
        limiter,
    };

    // Pause immediately
    control_handle.pause();

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Paused);
}

#[tokio::test]
async fn run_download_can_be_cancelled() {
    let harness = setup_seeder().await;
    let leecher_dir = TempDir::new().unwrap();
    let leecher_session = Session::new_with_opts(
        leecher_dir.path().to_path_buf(),
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

    let backend = TorrentBackend::new_with_session(leecher_session, 0.0);
    let store = Store::open_in_memory().unwrap();
    let mut job = Job::new(
        Source::Magnet(harness.magnet_uri.clone()),
        leecher_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    job.set_backend(BackendKind::Torrent);
    job.set_filename("test_file.bin");
    store.insert_job(&mut job).unwrap();

    let (control_handle, control_token) = ControlToken::new();
    let (progress_sink, _progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job,
        control: control_token,
        progress: progress_sink,
        store,
        limiter,
    };

    // Cancel immediately
    control_handle.cancel();

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
}
