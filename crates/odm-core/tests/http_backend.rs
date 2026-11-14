//! HTTP probe and single-stream download tests against a local server.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use odm_core::{
    Backend, BackendKind, ControlToken, Engine, HttpBackend, Job, JobContext, JobOptions, JobState,
    Limiter, ProgressSink, RunOutcome, Source, Store, TokenBucket,
};
use tempfile::TempDir;
use tokio::net::TcpListener;

const PAYLOAD: &[u8] = b"odm single stream test payload";

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/file.bin", get(file))
            .route("/named.bin", get(named))
            .route("/head-405.bin", any(no_head))
            .route("/range-405.bin", any(range))
            .route("/head-no-ranges.bin", any(head_without_ranges))
            .with_state(Arc::new(PAYLOAD.to_vec()));
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { addr, handle }
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

async fn file(State(payload): State<Arc<Vec<u8>>>) -> Response {
    response(&payload, None, None)
}

async fn named(State(payload): State<Arc<Vec<u8>>>) -> Response {
    response(
        &payload,
        Some("attachment; filename=\"report.pdf\""),
        Some("\"abc\""),
    )
}

async fn no_head(method: Method, State(payload): State<Arc<Vec<u8>>>) -> Response {
    if method == Method::HEAD {
        (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "GET")]).into_response()
    } else {
        response(&payload, None, None)
    }
}

async fn range(
    method: Method,
    headers: HeaderMap,
    State(payload): State<Arc<Vec<u8>>>,
) -> Response {
    if method == Method::HEAD {
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "GET")]).into_response();
    }
    if headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        == Some("bytes=0-0")
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes 0-0/{}", payload.len())).unwrap(),
        );
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("1"));
        headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        return (
            StatusCode::PARTIAL_CONTENT,
            headers,
            Body::from(payload[0..1].to_vec()),
        )
            .into_response();
    }
    response(&payload, None, None)
}

/// HEAD reports the size but not `Accept-Ranges`, yet a ranged GET works.
async fn head_without_ranges(
    method: Method,
    headers: HeaderMap,
    State(payload): State<Arc<Vec<u8>>>,
) -> Response {
    if method == Method::HEAD {
        return response(&payload, None, None);
    }
    range(method, headers, State(payload)).await
}

fn response(payload: &[u8], disposition: Option<&str>, etag: Option<&str>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&payload.len().to_string()).unwrap(),
    );
    if let Some(value) = disposition {
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(value).unwrap(),
        );
    }
    if let Some(value) = etag {
        headers.insert(header::ETAG, HeaderValue::from_str(value).unwrap());
    }
    (StatusCode::OK, headers, Body::from(payload.to_vec())).into_response()
}

#[tokio::test]
async fn probe_reads_metadata_and_filename() {
    let server = TestServer::start().await;
    let backend = HttpBackend::new().unwrap();

    let probe = backend
        .probe(
            &Source::Url(server.url("/named.bin")),
            &JobOptions::default(),
        )
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Http);
    assert_eq!(probe.filename, "report.pdf");
    assert_eq!(probe.total_bytes, Some(PAYLOAD.len() as u64));
    assert_eq!(probe.etag.as_deref(), Some("\"abc\""));
    assert!(!probe.supports_ranges);
}

#[tokio::test]
async fn probe_falls_back_when_head_is_not_allowed() {
    let server = TestServer::start().await;
    let backend = HttpBackend::new().unwrap();

    let probe = backend
        .probe(
            &Source::Url(server.url("/head-405.bin")),
            &JobOptions::default(),
        )
        .await
        .unwrap();

    assert_eq!(probe.filename, "head-405.bin");
    assert_eq!(probe.total_bytes, Some(PAYLOAD.len() as u64));
    assert!(!probe.supports_ranges);
}

#[tokio::test]
async fn probe_detects_ranges_from_partial_content() {
    let server = TestServer::start().await;
    let backend = HttpBackend::new().unwrap();

    let probe = backend
        .probe(
            &Source::Url(server.url("/range-405.bin")),
            &JobOptions::default(),
        )
        .await
        .unwrap();

    assert!(probe.supports_ranges);
    assert_eq!(probe.total_bytes, Some(PAYLOAD.len() as u64));
}

#[tokio::test]
async fn single_stream_download_writes_file_and_reports_progress() {
    let server = TestServer::start().await;
    let dir = TempDir::new().unwrap();
    let backend = HttpBackend::new().unwrap();
    let source = Source::Url(server.url("/file.bin"));

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

    let (_control_handle, control) = ControlToken::new();
    let (progress, mut updates) = ProgressSink::new();
    let context = JobContext {
        job: job.clone(),
        control,
        progress,
        store: Store::open_in_memory().unwrap(),
        limiter: Limiter::new(Arc::new(TokenBucket::new(0)), None),
    };

    let outcome = backend.run(context).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        PAYLOAD
    );
    assert!(!job.part_path().exists());

    let mut last = None;
    while let Some(update) = updates.recv().await {
        last = Some(update);
    }
    let last = last.unwrap();
    assert_eq!(last.downloaded_bytes, PAYLOAD.len() as u64);
    assert_eq!(last.total_bytes, Some(PAYLOAD.len() as u64));
}

#[tokio::test]
async fn engine_downloads_from_local_server() {
    let server = TestServer::start().await;
    let dir = TempDir::new().unwrap();
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();

    let job = engine
        .add(
            &server.url("/file.bin"),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();
    assert_eq!(job.state(), JobState::Queued);
    assert!(job.id().0 > 0);

    let job = engine.run(job.id()).await.unwrap();
    assert_eq!(job.state(), JobState::Completed);
    assert_eq!(job.downloaded_bytes(), PAYLOAD.len() as u64);
    assert_eq!(
        std::fs::read(dir.path().join("file.bin"))
            .unwrap()
            .as_slice(),
        PAYLOAD
    );
}

fn context_for(job: &Job, control: ControlToken) -> JobContext {
    let (progress, _updates) = ProgressSink::new();
    JobContext {
        job: job.clone(),
        control,
        progress,
        store: Store::open_in_memory().unwrap(),
        limiter: Limiter::new(Arc::new(TokenBucket::new(0)), None),
    }
}

async fn serve_routes(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn single_stream_retries_after_connection_reset() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let addr = serve_routes(Router::new().route(
        "/reset.bin",
        get(move || {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    // Promise the whole file, send a few bytes, then fail.
                    let chunks = futures_util::stream::iter(vec![
                        Ok(bytes_of(&PAYLOAD[..5])),
                        Err(std::io::Error::other("connection reset")),
                    ]);
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        header::CONTENT_LENGTH,
                        HeaderValue::from_str(&PAYLOAD.len().to_string()).unwrap(),
                    );
                    (StatusCode::OK, headers, Body::from_stream(chunks)).into_response()
                } else {
                    response(PAYLOAD, None, None)
                }
            }
        }),
    ))
    .await;
    let dir = TempDir::new().unwrap();
    let job = Job::new(
        Source::Url(format!("http://{addr}/reset.bin")),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let backend = HttpBackend::new()
        .unwrap()
        .with_retry_delays(vec![Duration::from_millis(10)]);
    let (_handle, control) = ControlToken::new();

    let outcome = backend.run(context_for(&job, control)).await.unwrap();

    assert_eq!(outcome, RunOutcome::Completed);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(
        std::fs::read(job.target_path()).unwrap().as_slice(),
        PAYLOAD
    );
}

fn bytes_of(data: &[u8]) -> axum::body::Bytes {
    axum::body::Bytes::copy_from_slice(data)
}

#[tokio::test]
async fn pause_during_backoff_returns_paused_quickly() {
    let addr = serve_routes(Router::new().route(
        "/down.bin",
        get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
    ))
    .await;
    let dir = TempDir::new().unwrap();
    let job = Job::new(
        Source::Url(format!("http://{addr}/down.bin")),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    let backend = HttpBackend::new()
        .unwrap()
        .with_retry_delays(vec![Duration::from_secs(30)]);
    let (handle, control) = ControlToken::new();
    let context = context_for(&job, control);
    let task = tokio::spawn(async move { backend.run(context).await });

    tokio::time::sleep(Duration::from_millis(400)).await;
    handle.pause();

    let outcome = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("pause must cut the 30 s backoff short")
        .unwrap()
        .unwrap();
    assert_eq!(outcome, RunOutcome::Paused);
}

#[tokio::test]
async fn stalled_server_times_out() {
    // Accepts connections and never answers.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let dir = TempDir::new().unwrap();
    let job = Job::new(
        Source::Url(format!("http://{addr}/stalled.bin")),
        dir.path().to_path_buf(),
        JobOptions {
            retry_count: Some(0),
            ..Default::default()
        },
    );
    let backend =
        HttpBackend::with_timeouts(Duration::from_secs(1), Duration::from_millis(300)).unwrap();
    let (_handle, control) = ControlToken::new();

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        backend.run(context_for(&job, control)),
    )
    .await
    .expect("a stalled server must not hang the job");

    assert!(result.is_err());
}

#[tokio::test]
async fn range_probe_falls_back_to_get_range_0_0() {
    let server = TestServer::start().await;
    let backend = HttpBackend::new().unwrap();

    let probe = backend
        .probe(
            &Source::Url(server.url("/head-no-ranges.bin")),
            &JobOptions::default(),
        )
        .await
        .unwrap();

    assert!(probe.supports_ranges);
    assert_eq!(probe.total_bytes, Some(PAYLOAD.len() as u64));
}
