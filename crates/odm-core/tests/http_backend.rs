//! HTTP probe and single-stream download tests against a local server.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use odm_core::{
    Backend, BackendKind, ControlToken, Engine, HttpBackend, Job, JobContext, JobOptions, JobState,
    ProgressSink, RunOutcome, Source, Store,
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
