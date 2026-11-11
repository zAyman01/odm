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

    match engine.pause(job.id()) {
        Err(EngineError::NotRunning(id)) => assert_eq!(id, job.id()),
        other => panic!("expected NotRunning, got {other:?}"),
    }
}
