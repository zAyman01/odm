//! Tests for HLS backend probing, variant selection, segment downloading, pausing, resuming, and DRM handling.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::backend::{Backend, ControlToken, JobContext, ProgressSink, RunOutcome};
use odm_core::job::{BackendKind, Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_media::hls::HlsBackend;
use odm_media::state::HlsResumeState;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::Notify;

const SEGMENT_SIZE: usize = 4096;

struct ServerState {
    seg0: Vec<u8>,
    seg1: Vec<u8>,
    seg2: Vec<u8>,
    seg1_requested: Arc<Notify>,
    seg1_fetch_count: AtomicUsize,
    init_fetch_count: AtomicUsize,
}

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
    state: Arc<ServerState>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let state = Arc::new(ServerState {
            seg0: vec![0xAA; SEGMENT_SIZE],
            seg1: vec![0xBB; SEGMENT_SIZE],
            seg2: vec![0xCC; SEGMENT_SIZE],
            seg1_requested: Arc::new(Notify::new()),
            seg1_fetch_count: AtomicUsize::new(0),
            init_fetch_count: AtomicUsize::new(0),
        });

        let app = Router::new()
            .route("/master.m3u8", get(master_playlist))
            .route("/480p.m3u8", get(playlist_480p))
            .route("/720p.m3u8", get(playlist_720p))
            .route("/drm.m3u8", get(drm_playlist))
            .route("/map.m3u8", get(map_playlist))
            .route("/live.m3u8", get(live_playlist))
            .route("/init.mp4", get(get_init))
            .route("/seg0.ts", get(get_seg0))
            .route("/seg1.ts", get(get_seg1))
            .route("/seg2.ts", get(get_seg2))
            .with_state(state.clone());

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            handle,
            state,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn master_playlist() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=854x480,URI=\"480p.m3u8\"\n\
480p.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720,URI=\"720p.m3u8\"\n\
720p.m3u8\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn playlist_480p() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:10.0,\n\
seg0.ts\n\
#EXTINF:10.0,\n\
seg1.ts\n\
#EXTINF:10.0,\n\
seg2.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn playlist_720p() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:10.0,\n\
seg0.ts\n\
#EXTINF:10.0,\n\
seg1.ts\n\
#EXTINF:10.0,\n\
seg2.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn drm_playlist() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://drm.example.com/key\"\n\
#EXTINF:10.0,\n\
seg0.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn map_playlist() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:7\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:0\n\
#EXT-X-MAP:URI=\"init.mp4\"\n\
#EXTINF:10.0,\n\
seg0.ts\n\
#EXTINF:10.0,\n\
seg1.ts\n\
#EXT-X-ENDLIST\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn live_playlist() -> Response {
    let body = "#EXTM3U\n\
#EXT-X-VERSION:3\n\
#EXT-X-TARGETDURATION:10\n\
#EXT-X-MEDIA-SEQUENCE:100\n\
#EXTINF:10.0,\n\
seg0.ts\n\
#EXTINF:10.0,\n\
seg1.ts\n";
    (
        [(CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body.to_string(),
    )
        .into_response()
}

async fn get_init(State(state): State<Arc<ServerState>>) -> Response {
    state.init_fetch_count.fetch_add(1, Ordering::SeqCst);
    (StatusCode::OK, Bytes::from(vec![0x99; 1024])).into_response()
}

async fn get_seg0(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/MP2T"));
    (StatusCode::OK, headers, Bytes::from(state.seg0.clone())).into_response()
}

async fn get_seg1(State(state): State<Arc<ServerState>>) -> Response {
    state.seg1_fetch_count.fetch_add(1, Ordering::SeqCst);
    state.seg1_requested.notify_waiters();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/MP2T"));
    (StatusCode::OK, headers, Bytes::from(state.seg1.clone())).into_response()
}

async fn get_seg2(State(state): State<Arc<ServerState>>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("video/MP2T"));
    (StatusCode::OK, headers, Bytes::from(state.seg2.clone())).into_response()
}

#[tokio::test]
async fn probe_master_playlist_returns_hls_and_qualities() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/master.m3u8"));

    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Hls);
    assert!(!probe.filename.is_empty());

    let qualities = backend.probe_qualities(&source).await.unwrap();
    assert_eq!(qualities.len(), 2);

    let q480 = qualities
        .iter()
        .find(|q| q.id == "480p" || q.resolution.as_deref() == Some("854x480"))
        .expect("480p rendition must be present");
    assert_eq!(q480.bandwidth, Some(800000));

    let q720 = qualities
        .iter()
        .find(|q| q.id == "720p" || q.resolution.as_deref() == Some("1280x720"))
        .expect("720p rendition must be present");
    assert_eq!(q720.bandwidth, Some(2000000));
}

#[tokio::test]
async fn probe_media_playlist_returns_estimated_size_and_filename() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/720p.m3u8"));

    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Hls);
    assert!(probe.filename.starts_with("720p"));
    // 3 segments * 4096 bytes = 12288
    assert_eq!(probe.total_bytes, Some(3 * SEGMENT_SIZE as u64));
}

#[tokio::test]
async fn download_media_playlist_concatenates_segments() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/720p.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let expected_data = [
        server.state.seg0.as_slice(),
        server.state.seg1.as_slice(),
        server.state.seg2.as_slice(),
    ]
    .concat();

    let target_path = job.target_path();
    assert!(
        target_path.exists(),
        "Target path {} does not exist",
        target_path.display()
    );
    let downloaded_data = std::fs::read(&target_path).unwrap();
    assert_eq!(downloaded_data.len(), expected_data.len());
    assert_eq!(
        Sha256::digest(&downloaded_data),
        Sha256::digest(&expected_data)
    );

    // Verify progress was reported
    let mut total_reported = 0;
    while let Ok(update) = progress_rx.try_recv() {
        total_reported = update.downloaded_bytes;
    }
    assert_eq!(total_reported, expected_data.len() as u64);
}

#[tokio::test]
async fn pause_and_resume_download() {
    let server = TestServer::start().await;
    let backend = Arc::new(HlsBackend::new(reqwest::Client::new(), None));
    let source = Source::Url(server.url("/720p.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();
    let job_id = job.id();

    let (control_handle, control) = ControlToken::new();
    let (progress, _) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter: limiter.clone(),
    };

    // Pause after seg1 starts or before seg2
    let seg1_notif = server.state.seg1_requested.clone();
    let handle_clone = control_handle.clone();
    tokio::spawn(async move {
        seg1_notif.notified().await;
        // Pause right after seg1 requested
        handle_clone.pause();
    });

    let backend_clone = backend.clone();
    let outcome = backend_clone.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Paused);

    // Verify backend_state in store
    let state_str = store
        .get_backend_state(job_id)
        .unwrap()
        .expect("backend_state should be persisted on pause");
    let resume_state: HlsResumeState = serde_json::from_str(&state_str).unwrap();
    assert!(
        !resume_state.completed_segments.is_empty(),
        "At least one segment should have completed before pause"
    );
    assert!(
        resume_state.completed_segments.len() < 3,
        "Should have paused before all segments finished"
    );

    // Resume the job
    control_handle.resume();
    let (_control_handle2, control2) = ControlToken::new();
    let (progress2, _) = ProgressSink::new();

    let resumed_job = store.get_job(job_id).unwrap();
    let ctx2 = JobContext {
        job: resumed_job,
        control: control2,
        progress: progress2,
        store: store.clone(),
        limiter,
    };

    let outcome2 = backend.run(ctx2).await.unwrap();
    assert_eq!(outcome2, RunOutcome::Completed);

    let expected_data = [
        server.state.seg0.as_slice(),
        server.state.seg1.as_slice(),
        server.state.seg2.as_slice(),
    ]
    .concat();
    let downloaded_data = std::fs::read(job.target_path()).unwrap();
    assert_eq!(
        Sha256::digest(&downloaded_data),
        Sha256::digest(&expected_data)
    );
}

#[tokio::test]
async fn cancel_download() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/720p.m3u8"));
    let temp_dir = TempDir::new().unwrap();

    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (control_handle, control) = ControlToken::new();
    let (progress, _) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    control_handle.cancel();

    let ctx = JobContext {
        job,
        control,
        progress,
        store,
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Cancelled);
}

#[tokio::test]
async fn drm_detection_fails_probe() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/drm.m3u8"));

    let probe_err = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap_err();

    let err_msg = probe_err.to_string();
    assert!(
        err_msg.contains("DRM") || err_msg.contains("not supported"),
        "Error message should mention DRM or not supported: {err_msg}"
    );
}

#[tokio::test]
async fn hls_ext_x_map_init_segment_is_prepended() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(server.url("/map.m3u8"));
    let temp_dir = TempDir::new().unwrap();
    let mut job = Job::new(
        source.clone(),
        temp_dir.path().to_path_buf(),
        JobOptions::default(),
    );
    job.apply_probe(
        backend
            .probe(&source, &JobOptions::default())
            .await
            .unwrap(),
    );
    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();
    let (_control_handle, control) = ControlToken::new();
    let (progress, _progress_rx) = ProgressSink::new();

    let outcome = backend
        .run(JobContext {
            job: job.clone(),
            control,
            progress,
            store,
            limiter: Limiter::new(Arc::new(TokenBucket::new(0)), None),
        })
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    let expected = [
        vec![0x99; 1024].as_slice(),
        server.state.seg0.as_slice(),
        server.state.seg1.as_slice(),
    ]
    .concat();
    assert_eq!(std::fs::read(job.target_path()).unwrap(), expected);
    assert_eq!(server.state.init_fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn hls_live_playlist_is_rejected() {
    let server = TestServer::start().await;
    let backend = HlsBackend::new(reqwest::Client::new(), None);

    let error = backend
        .probe(
            &Source::Url(server.url("/live.m3u8")),
            &JobOptions::default(),
        )
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("live streams are not supported"),
        "got: {error}"
    );
}

const MANY_SEGMENTS: usize = 20;

async fn many_playlist() -> Response {
    let mut body = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n");
    for n in 0..MANY_SEGMENTS {
        body.push_str(&format!("#EXTINF:1.0,\nmany/{n}\n"));
    }
    body.push_str("#EXT-X-ENDLIST\n");
    ([(CONTENT_TYPE, "application/vnd.apple.mpegurl")], body).into_response()
}

/// Segment `n` holds 100 bytes of the value `n + 1`; the segment named by
/// `failing` answers 500.
async fn many_segment(Path(n): Path<usize>, State(failing): State<Arc<AtomicUsize>>) -> Response {
    if failing.load(Ordering::SeqCst) == n {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    vec![n as u8 + 1; 100].into_response()
}

async fn serve_many(failing: Arc<AtomicUsize>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/many.m3u8", get(many_playlist))
        .route("/many/{n}", get(many_segment))
        .with_state(failing);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn prepare_many(addr: SocketAddr, dir: &TempDir) -> (HlsBackend, Job, Store) {
    let backend = HlsBackend::new(reqwest::Client::new(), None);
    let source = Source::Url(format!("http://{addr}/many.m3u8"));
    let mut job = Job::new(
        source.clone(),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    job.apply_probe(
        backend
            .probe(&source, &JobOptions::default())
            .await
            .unwrap(),
    );
    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();
    (backend, job, store)
}

fn many_context(job: &Job, store: &Store) -> (odm_core::backend::ControlHandle, JobContext) {
    let (handle, control) = ControlToken::new();
    let (progress, _) = ProgressSink::new();
    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter: Limiter::new(Arc::new(TokenBucket::new(0)), None),
    };
    (handle, ctx)
}

#[tokio::test]
async fn hls_state_not_rewritten_per_segment() {
    let addr = serve_many(Arc::new(AtomicUsize::new(usize::MAX))).await;
    let dir = TempDir::new().unwrap();
    let (backend, job, store) = prepare_many(addr, &dir).await;
    let (_handle, ctx) = many_context(&job, &store);

    let before = store.total_changes().unwrap();
    let outcome = backend.run(ctx).await.unwrap();
    let writes = store.total_changes().unwrap() - before;

    assert_eq!(outcome, RunOutcome::Completed);
    assert!(
        writes <= 4,
        "{writes} state writes for {MANY_SEGMENTS} segments"
    );
}

#[tokio::test]
async fn resume_after_a_failure_does_not_duplicate_unsaved_segments() {
    let failing = Arc::new(AtomicUsize::new(15));
    let addr = serve_many(failing.clone()).await;
    let dir = TempDir::new().unwrap();
    let (backend, job, store) = prepare_many(addr, &dir).await;

    let (_handle, ctx) = many_context(&job, &store);
    assert!(backend.run(ctx).await.is_err());

    failing.store(usize::MAX, Ordering::SeqCst);
    let (_handle, ctx) = many_context(&store.get_job(job.id()).unwrap(), &store);
    let outcome = backend.run(ctx).await.unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    let expected: Vec<u8> = (0..MANY_SEGMENTS)
        .flat_map(|n| vec![n as u8 + 1; 100])
        .collect();
    assert_eq!(std::fs::read(job.target_path()).unwrap(), expected);
}
