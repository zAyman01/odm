//! Segmented transfer, resume, retry and split tests against a local server.
#![allow(clippy::unwrap_used, clippy::expect_used, deprecated)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures_util::stream;
use odm_core::{
    Backend, BackendKind, ControlToken, HttpBackend, Job, JobContext, JobOptions, Limiter,
    ProbeResult, ProgressSink, RunOutcome, Segment, Source, Store, TokenBucket,
};
use tempfile::TempDir;
use tokio::net::TcpListener;

const BLOCK: usize = 64 * 1024;

fn payload(size: usize) -> Arc<Vec<u8>> {
    Arc::new((0..size).map(|index| (index % 251) as u8).collect())
}

struct TestServer {
    addr: SocketAddr,
    state: Arc<ServerState>,
    handle: tokio::task::JoinHandle<()>,
}

struct ServerState {
    payload: Mutex<Vec<u8>>,
    etag: Mutex<String>,
    supports_ranges: bool,
    slow: bool,
    fail_next: AtomicUsize,
    ranges: Mutex<Vec<String>>,
    if_ranges: Mutex<Vec<String>>,
}

impl TestServer {
    async fn start(payload: Vec<u8>, supports_ranges: bool, slow: bool) -> Self {
        let state = Arc::new(ServerState {
            payload: Mutex::new(payload),
            etag: Mutex::new("\"v1\"".to_string()),
            supports_ranges,
            slow,
            fail_next: AtomicUsize::new(0),
            ranges: Mutex::new(Vec::new()),
            if_ranges: Mutex::new(Vec::new()),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/file.bin", any(serve))
            .with_state(state.clone());
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            state,
            handle,
        }
    }

    fn url(&self) -> String {
        format!("http://{}/file.bin", self.addr)
    }

    fn set_etag(&self, etag: &str) {
        *self.state.etag.lock().unwrap() = etag.to_string();
    }

    fn set_payload(&self, payload: Vec<u8>) {
        *self.state.payload.lock().unwrap() = payload;
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve(
    method: Method,
    headers: HeaderMap,
    State(state): State<Arc<ServerState>>,
) -> Response {
    if method == Method::HEAD {
        let mut response = HeaderMap::new();
        response.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&state.payload.lock().unwrap().len().to_string()).unwrap(),
        );
        response.insert(
            header::ETAG,
            HeaderValue::from_str(&state.etag.lock().unwrap()).unwrap(),
        );
        response.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        );
        if state.supports_ranges {
            response.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        }
        return (StatusCode::OK, response).into_response();
    }

    if let Some(range) = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
    {
        state.ranges.lock().unwrap().push(range.to_string());
    }
    if let Some(if_range) = headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
    {
        state.if_ranges.lock().unwrap().push(if_range.to_string());
    }
    if state
        .fail_next
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
            value.checked_sub(1)
        })
        .is_ok()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.split_once('-'))
        .and_then(|(start, end)| Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?)));

    let etag = state.etag.lock().unwrap().clone();
    if let Some(if_range) = headers
        .get(header::IF_RANGE)
        .and_then(|value| value.to_str().ok())
        && if_range != etag
    {
        return full(&state, StatusCode::OK);
    }

    match range {
        Some((start, end)) if state.supports_ranges => {
            let payload = state.payload.lock().unwrap().clone();
            let end = end.min(payload.len() - 1);
            let slice = payload[start..=end].to_vec();
            let mut response = HeaderMap::new();
            response.insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end}/{}", payload.len())).unwrap(),
            );
            response.insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&slice.len().to_string()).unwrap(),
            );
            response.insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
            let delay = if state.slow && start == 0 { 5 } else { 0 };
            (StatusCode::PARTIAL_CONTENT, response, body(slice, delay)).into_response()
        }
        _ => full(&state, StatusCode::OK),
    }
}

fn full(state: &ServerState, status: StatusCode) -> Response {
    let mut response = HeaderMap::new();
    response.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&state.payload.lock().unwrap().len().to_string()).unwrap(),
    );
    response.insert(
        header::ETAG,
        HeaderValue::from_str(&state.etag.lock().unwrap()).unwrap(),
    );
    (
        status,
        response,
        body(
            state.payload.lock().unwrap().clone(),
            if state.slow { 5 } else { 0 },
        ),
    )
        .into_response()
}

fn body(bytes: Vec<u8>, delay_ms: u64) -> Body {
    if delay_ms == 0 {
        return Body::from(bytes);
    }

    let chunks = stream::unfold(bytes, move |mut bytes| async move {
        if bytes.is_empty() {
            return None;
        }
        let rest = bytes.split_off(bytes.len().min(BLOCK));
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        Some((Ok::<Vec<u8>, std::io::Error>(bytes), rest))
    });
    Body::from_stream(chunks)
}

fn probe(server: &TestServer, supports_ranges: bool) -> ProbeResult {
    ProbeResult {
        backend: BackendKind::Http,
        filename: "file.bin".to_string(),
        total_bytes: Some(server.state.payload.lock().unwrap().len() as u64),
        supports_ranges,
        etag: Some(server.state.etag.lock().unwrap().clone()),
        last_modified: None,
        content_type: Some("application/octet-stream".to_string()),
    }
}

fn ready_job(server: &TestServer, store: &Store, dir: &Path, segments: Option<u32>) -> Job {
    let mut job = Job::new(
        Source::Url(server.url()),
        dir.to_path_buf(),
        JobOptions {
            segments,
            ..JobOptions::default()
        },
    );
    job.apply_probe(probe(server, server.state.supports_ranges));
    store.insert_job(&mut job).unwrap();
    job
}

fn context(job: &Job, store: &Store, control: ControlToken, progress: ProgressSink) -> JobContext {
    JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter: Limiter::new(Arc::new(TokenBucket::new(0)), job.options().speed_limit),
    }
}

#[tokio::test]
async fn segmented_download_writes_ranges_and_whole_file() {
    // Four segments of at least MIN_SEGMENT_BYTES each.
    let payload = payload(BLOCK * 64);
    let server = TestServer::start(payload.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), Some(4));

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = HttpBackend::new()
        .unwrap()
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
    assert!(!job.part_path().exists());
    assert!(store.list_segments(job.id()).unwrap().is_empty());
    assert!(server.state.ranges.lock().unwrap().len() >= 4);
}

#[tokio::test]
async fn dynamic_split_adds_a_worker_segment() {
    let payload = payload(BLOCK * 96);
    let server = TestServer::start(payload.as_ref().clone(), true, true).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), Some(2));

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = HttpBackend::new()
        .unwrap()
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
    let ranges = server.state.ranges.lock().unwrap();
    assert!(
        ranges.len() > 2,
        "expected dynamic split, saw ranges {ranges:?}"
    );
}

#[tokio::test]
async fn pause_and_resume_completes_the_file() {
    let payload = payload(BLOCK * 16);
    let server = TestServer::start(payload.as_ref().clone(), true, true).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), Some(4));
    let backend = HttpBackend::new().unwrap();

    let (handle, control) = ControlToken::new();
    let (progress, mut updates) = ProgressSink::new();
    let runner = backend.clone();
    let task = tokio::spawn({
        let context = context(&job, &store, control, progress);
        async move { runner.run(context).await }
    });

    updates.recv().await.unwrap();
    handle.pause();
    let outcome = task.await.unwrap().unwrap();

    assert_eq!(outcome, RunOutcome::Paused);
    assert!(job.part_path().exists());
    let saved: u64 = store
        .list_segments(job.id())
        .unwrap()
        .iter()
        .map(|segment| segment.downloaded)
        .sum();
    assert!(saved > 0);
    assert!(saved < payload.len() as u64);

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = backend
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
    assert!(store.list_segments(job.id()).unwrap().is_empty());
}

#[tokio::test]
async fn changed_etag_restarts_from_zero() {
    let payload = payload(BLOCK * 8);
    let server = TestServer::start(payload.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();

    let mut job = Job::new(
        Source::Url(server.url()),
        dir.path().to_path_buf(),
        JobOptions {
            segments: Some(4),
            ..JobOptions::default()
        },
    );
    let mut original = probe(&server, true);
    original.etag = Some("\"v1\"".to_string());
    job.apply_probe(original);
    store.insert_job(&mut job).unwrap();

    let half = payload.len() / 2;
    std::fs::write(job.part_path(), &payload[..half]).unwrap();
    store
        .replace_segments(
            job.id(),
            &[
                Segment {
                    index: 0,
                    start: 0,
                    end: (half - 1) as u64,
                    downloaded: half as u64,
                },
                Segment {
                    index: 1,
                    start: half as u64,
                    end: (payload.len() - 1) as u64,
                    downloaded: 0,
                },
            ],
        )
        .unwrap();
    server.set_etag("\"v2\"");

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = HttpBackend::new()
        .unwrap()
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
    assert!(
        server
            .state
            .if_ranges
            .lock()
            .unwrap()
            .iter()
            .any(|value| value == "\"v1\"")
    );
}

#[tokio::test]
async fn retries_transient_server_errors() {
    let payload = payload(BLOCK * 2);
    let server = TestServer::start(payload.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), Some(2));
    server.state.fail_next.store(1, Ordering::SeqCst);

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = HttpBackend::new()
        .unwrap()
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
}

#[tokio::test]
async fn single_stream_pause_discards_the_partial_file() {
    let payload = payload(BLOCK * 8);
    let server = TestServer::start(payload.as_ref().clone(), false, true).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), None);
    let backend = HttpBackend::new().unwrap();

    let (handle, control) = ControlToken::new();
    let (progress, mut updates) = ProgressSink::new();
    let runner = backend.clone();
    let task = tokio::spawn({
        let context = context(&job, &store, control, progress);
        async move { runner.run(context).await }
    });

    updates.recv().await.unwrap();
    handle.pause();
    let outcome = task.await.unwrap().unwrap();

    assert_eq!(outcome, RunOutcome::Paused);
    assert!(!job.part_path().exists());
}

#[tokio::test]
async fn retry_count_option_limits_the_attempts_after_server_errors() {
    let gets = Arc::new(AtomicUsize::new(0));
    let counter = gets.clone();
    let app = Router::new().route(
        "/flaky.bin",
        any(move |method: Method| {
            let counter = counter.clone();
            async move {
                if method == Method::HEAD {
                    return (
                        StatusCode::OK,
                        [
                            (header::CONTENT_LENGTH, "131072"),
                            (header::ACCEPT_RANGES, "bytes"),
                        ],
                    )
                        .into_response();
                }
                counter.fetch_add(1, Ordering::SeqCst);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let dir = TempDir::new().unwrap();
    let engine = odm_core::Engine::new(Store::open_in_memory().unwrap()).unwrap();
    let job = engine
        .add(
            &format!("http://{addr}/flaky.bin"),
            dir.path().to_path_buf(),
            JobOptions {
                segments: Some(1),
                retry_count: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

    let result = tokio::time::timeout(Duration::from_secs(8), engine.run(job.id()))
        .await
        .expect("retry_count must cap the retries");

    assert!(result.is_err());
    assert_eq!(gets.load(Ordering::SeqCst), 2);
}

/// A job whose saved segments say `done_each` bytes of both halves are on disk.
fn job_with_saved_halves(
    server: &TestServer,
    store: &Store,
    dir: &Path,
    old: &[u8],
    write_part: bool,
    etag: &str,
) -> Job {
    let mut job = Job::new(
        Source::Url(server.url()),
        dir.to_path_buf(),
        JobOptions {
            segments: Some(2),
            ..JobOptions::default()
        },
    );
    let mut original = probe(server, true);
    original.etag = Some(etag.to_string());
    job.apply_probe(original);
    store.insert_job(&mut job).unwrap();

    let half = old.len() / 2;
    let quarter = half / 2;
    if write_part {
        let mut part = vec![0_u8; old.len()];
        part[..quarter].copy_from_slice(&old[..quarter]);
        part[half..half + quarter].copy_from_slice(&old[half..half + quarter]);
        std::fs::write(job.part_path(), part).unwrap();
    }
    store
        .replace_segments(
            job.id(),
            &[
                Segment {
                    index: 0,
                    start: 0,
                    end: (half - 1) as u64,
                    downloaded: quarter as u64,
                },
                Segment {
                    index: 1,
                    start: half as u64,
                    end: (old.len() - 1) as u64,
                    downloaded: quarter as u64,
                },
            ],
        )
        .unwrap();
    job
}

async fn run_to_end(job: &Job, store: &Store) -> Result<RunOutcome, odm_core::BackendError> {
    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    HttpBackend::new()
        .unwrap()
        .run(context(job, store, control, progress))
        .await
}

#[tokio::test]
async fn changed_file_after_restart_restarts_cleanly() {
    let old = payload(BLOCK * 8);
    let new: Vec<u8> = old.iter().map(|byte| byte.wrapping_add(1)).collect();
    let server = TestServer::start(old.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = job_with_saved_halves(&server, &store, dir.path(), &old, true, "\"v1\"");
    server.set_payload(new.clone());
    server.set_etag("\"v2\"");

    let outcome = run_to_end(&job, &store).await.unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(std::fs::read(job.target_path()).unwrap(), new);
}

#[tokio::test]
async fn weak_etag_is_not_sent_in_if_range() {
    let data = payload(BLOCK * 8);
    let server = TestServer::start(data.as_ref().clone(), true, false).await;
    server.set_etag("W/\"abc\"");
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = job_with_saved_halves(&server, &store, dir.path(), &data, true, "W/\"abc\"");

    let outcome = run_to_end(&job, &store).await.unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert!(server.state.if_ranges.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        data.as_slice()
    );
}

#[tokio::test]
async fn missing_part_file_resets_segments() {
    let data = payload(BLOCK * 8);
    let server = TestServer::start(data.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = job_with_saved_halves(&server, &store, dir.path(), &data, false, "\"v1\"");

    let outcome = run_to_end(&job, &store).await.unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        data.as_slice()
    );
}

#[tokio::test]
async fn small_file_uses_one_connection() {
    let payload = payload(100 * 1024);
    let server = TestServer::start(payload.as_ref().clone(), true, false).await;
    let store = Store::open_in_memory().unwrap();
    let dir = TempDir::new().unwrap();
    let job = ready_job(&server, &store, dir.path(), Some(8));

    let (_handle, control) = ControlToken::new();
    let (progress, _updates) = ProgressSink::new();
    let outcome = HttpBackend::new()
        .unwrap()
        .run(context(&job, &store, control, progress))
        .await
        .unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        payload.as_slice()
    );
    assert_eq!(server.state.ranges.lock().unwrap().len(), 1);
}
