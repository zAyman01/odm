//! Bounded concurrent queue execution.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_core::{Engine, JobOptions, JobState, Store};
use tempfile::TempDir;
use tokio::net::TcpListener;

const PAYLOAD: &[u8] = b"odm queue payload";

struct TestServer {
    addr: SocketAddr,
    _active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct ServerState {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

impl TestServer {
    async fn start() -> Self {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let state = ServerState {
            active: active.clone(),
            max_active: max_active.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/file.bin", get(file))
            .with_state(state);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            _active: active,
            max_active,
            handle,
        }
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

async fn file(State(state): State<ServerState>) -> Response {
    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.max_active.fetch_max(active, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(30)).await;
    state.active.fetch_sub(1, Ordering::SeqCst);

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
async fn run_queue_respects_the_concurrency_limit() {
    let server = TestServer::start().await;
    let store = Store::open_in_memory().unwrap();
    let engine = Engine::new(store).unwrap();
    let dir = TempDir::new().unwrap();

    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(
            engine
                .add(
                    &server.url(),
                    dir.path().to_path_buf(),
                    JobOptions::default(),
                )
                .unwrap()
                .id(),
        );
    }
    engine.store().set_queue_limit(1, 2).unwrap();

    let results = engine.run_queue("default").await.unwrap();
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|result| result.is_ok()));

    for id in ids {
        let job = engine.store().get_job(id).unwrap();
        assert_eq!(job.state(), JobState::Completed);
    }
    assert!(server.max_active.load(Ordering::SeqCst) <= 2);
}
