//! Engine pause, resume, retry and crash recovery.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::{Engine, EngineError, JobOptions, JobState, Store};
use tempfile::TempDir;
use tokio::net::TcpListener;

const PAYLOAD: &[u8] = b"odm engine resume payload";

struct TestServer {
    addr: SocketAddr,
    fail: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

struct ServerState {
    fail: Arc<AtomicBool>,
}

impl TestServer {
    async fn start() -> Self {
        let fail = Arc::new(AtomicBool::new(false));
        let state = Arc::new(ServerState { fail: fail.clone() });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/file.bin", get(file))
            .with_state(state);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { addr, fail, handle }
    }

    fn url(&self) -> String {
        format!("http://{}/file.bin", self.addr)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn file(method: Method, State(state): State<Arc<ServerState>>) -> Response {
    if state.fail.load(Ordering::SeqCst) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if method == Method::HEAD {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_LENGTH, PAYLOAD.len().to_string()),
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        ],
        Body::from(PAYLOAD.to_vec()),
    )
        .into_response()
}

#[tokio::test]
async fn engine_recovers_interrupted_jobs_on_start() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("odm.sqlite");

    let id = {
        let store = Store::open(&path).unwrap();
        let mut job = odm_core::Job::new(
            odm_core::Source::Url("https://example.com/file.bin".to_string()),
            dir.path().to_path_buf(),
            JobOptions::default(),
        );
        store.insert_job(&mut job).unwrap();
        job.transition_to(JobState::Probing).unwrap();
        job.transition_to(JobState::Downloading).unwrap();
        store.update_job(&job).unwrap();
        job.id()
    };

    let store = Store::open(&path).unwrap();
    let _engine = Engine::new(store).unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.get_job(id).unwrap().state(), JobState::Paused);
}

#[tokio::test]
async fn engine_resumes_a_paused_job() {
    let server = TestServer::start().await;
    let dir = TempDir::new().unwrap();
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();

    let mut job = engine
        .add(
            &server.url(),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();
    job.transition_to(JobState::Probing).unwrap();
    job.transition_to(JobState::Downloading).unwrap();
    job.transition_to(JobState::Paused).unwrap();
    engine.store().update_job(&job).unwrap();

    let job = engine.run(job.id()).await.unwrap();
    assert_eq!(job.state(), JobState::Completed);
    assert_eq!(
        std::fs::read(dir.path().join("file.bin"))
            .unwrap()
            .as_slice(),
        PAYLOAD
    );
}

#[tokio::test]
async fn engine_retries_a_failed_job() {
    let server = TestServer::start().await;
    let dir = TempDir::new().unwrap();
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();

    let job = engine
        .add(
            &server.url(),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();
    server.fail.store(true, Ordering::SeqCst);

    let error = engine.run(job.id()).await.unwrap_err();
    assert!(matches!(error, EngineError::Backend(_)));
    assert_eq!(
        engine.store().get_job(job.id()).unwrap().state(),
        JobState::Failed
    );

    server.fail.store(false, Ordering::SeqCst);
    let job = engine.run(job.id()).await.unwrap();
    assert_eq!(job.state(), JobState::Completed);
}

#[tokio::test]
async fn pause_controls_a_running_job() {
    let server = TestServer::start().await;
    let dir = TempDir::new().unwrap();
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();

    let job = engine
        .add(
            &server.url(),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();

    let runner = engine.clone();
    let job_id = job.id();
    let running = tokio::spawn(async move { runner.run(job_id).await });
    for _ in 0..200 {
        if engine.store().get_job(job.id()).unwrap().state() == JobState::Probing {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    engine.pause(job.id()).unwrap();
    let job = running.await.unwrap().unwrap();

    assert_eq!(job.state(), JobState::Paused);
    assert_eq!(
        engine.store().get_job(job.id()).unwrap().state(),
        JobState::Paused
    );
}

#[tokio::test]
async fn pausing_a_job_that_is_not_running_is_reported() {
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();
    let dir = TempDir::new().unwrap();
    let job = engine
        .add(
            "https://example.com/file.bin",
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();
    // A queued job can be paused now, so use a job that is finished instead.
    engine.cancel(job.id()).unwrap();

    match engine.pause(job.id()) {
        Err(EngineError::NotRunning(id)) => assert_eq!(id, job.id()),
        other => panic!("expected NotRunning, got {other:?}"),
    }
}

struct StateBackend {
    seen: Arc<std::sync::Mutex<Option<String>>>,
}

#[async_trait::async_trait]
impl odm_core::backend::Backend for StateBackend {
    fn kind(&self) -> odm_core::BackendKind {
        odm_core::BackendKind::Torrent
    }

    async fn probe(
        &self,
        _source: &odm_core::Source,
        _opts: &JobOptions,
    ) -> Result<odm_core::ProbeResult, odm_core::backend::BackendError> {
        Ok(odm_core::ProbeResult {
            backend: odm_core::BackendKind::Torrent,
            filename: "state.bin".to_string(),
            total_bytes: Some(10),
            supports_ranges: true,
            etag: None,
            last_modified: None,
            content_type: None,
        })
    }

    async fn run(
        &self,
        ctx: odm_core::backend::JobContext,
    ) -> Result<odm_core::backend::RunOutcome, odm_core::backend::BackendError> {
        match ctx.store.get_backend_state(ctx.job.id())? {
            None => {
                ctx.store
                    .set_backend_state(ctx.job.id(), r#"{"torrent_id":7}"#)?;
                Ok(odm_core::backend::RunOutcome::Paused)
            }
            Some(state) => {
                *self.seen.lock().unwrap() = Some(state);
                Ok(odm_core::backend::RunOutcome::Completed)
            }
        }
    }
}

#[tokio::test]
async fn pause_resume_keeps_backend_state() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(Store::open_in_memory().unwrap()).unwrap();
    let seen = Arc::new(std::sync::Mutex::new(None));
    engine.register_backend(Arc::new(StateBackend { seen: seen.clone() }));

    let job = engine
        .add_download(
            odm_core::Source::Magnet("magnet:?xt=urn:btih:dummy".to_string()),
            dir.path().to_path_buf(),
            JobOptions::default(),
        )
        .unwrap();

    let paused = engine.run(job.id()).await.unwrap();
    assert_eq!(paused.state(), JobState::Paused);
    let done = engine.run(job.id()).await.unwrap();
    assert_eq!(done.state(), JobState::Completed);

    assert_eq!(seen.lock().unwrap().as_deref(), Some(r#"{"torrent_id":7}"#));
}
