//! REST API routes test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::Router;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, CreateQueueRequest, DownloadDetailDto, HealthResponse,
    ListDownloadsResponse, ListQueuesResponse, ProbeRequest, ProbeResponse, SettingsDto,
    UpdateSettingsRequest,
};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

async fn test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = vec![b'y'; 128 * 1024];

    let app = Router::new().route(
        "/sample.bin",
        get(move || {
            let p = payload.clone();
            async move {
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, p.len().to_string().leak() as &str),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    p,
                )
                    .into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}/sample.bin")
}

#[tokio::test]
async fn full_rest_lifecycle_test() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    let test_url = test_server().await;

    // 1. Health
    let req = axum::http::Request::builder()
        .uri("/api/v1/health")
        .header(HOST, "127.0.0.1")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let health: HealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "ok");

    // 2. Probe
    let probe_body = serde_json::to_vec(&ProbeRequest {
        url: test_url.clone(),
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let probe: ProbeResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(probe.filename, "sample.bin");

    // 3. Add download
    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: test_url,
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: Some("sample.bin".to_string()),
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
    assert_eq!(add_resp.id, 1);

    // 4. List downloads
    let req = axum::http::Request::builder()
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let list_resp: ListDownloadsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list_resp.downloads.len(), 1);
    assert_eq!(list_resp.downloads[0].id, 1);

    // 5. Get detail
    let req = axum::http::Request::builder()
        .uri("/api/v1/downloads/1")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let detail_resp: DownloadDetailDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(detail_resp.item.id, 1);

    // 6. Pause, resume, cancel
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads/1/cancel")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 7. Queues
    let queue_body = serde_json::to_vec(&CreateQueueRequest {
        name: "fast".to_string(),
        max_concurrent: 5,
        schedule_start: None,
        schedule_stop: None,
        schedule_days: None,
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/queues")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(queue_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let req = axum::http::Request::builder()
        .uri("/api/v1/queues")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let q_list: ListQueuesResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(q_list.queues.iter().any(|q| q.name == "fast"));

    // 8. Settings
    let update_settings = serde_json::to_vec(&UpdateSettingsRequest {
        default_segments: Some(12),
        ..Default::default()
    })
    .unwrap();
    let req = axum::http::Request::builder()
        .method("PATCH")
        .uri("/api/v1/settings")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(update_settings))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let settings: SettingsDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(settings.default_segments, 12);
}

#[tokio::test]
async fn add_forwards_custom_headers_to_origin() {
    let seen: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let recorder = seen.clone();
    let origin = Router::new().route(
        "/headers.bin",
        get(move |headers: axum::http::HeaderMap| {
            let recorder = recorder.clone();
            async move {
                if let Some(value) = headers.get("x-test").and_then(|v| v.to_str().ok()) {
                    recorder.lock().unwrap().push(value.to_string());
                }
                (
                    StatusCode::OK,
                    [(CONTENT_TYPE, "application/octet-stream")],
                    vec![b'h'; 1024],
                )
                    .into_response()
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, origin).await.unwrap();
    });

    let dir = tempdir().unwrap();
    let engine = Engine::new(Store::open_in_memory().unwrap()).unwrap();
    let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret"));
    let app = build_router_with_state(state.clone());

    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: format!("http://{addr}/headers.bin"),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: None,
        queue: None,
        category: None,
        options: Some(odm_api::JobOptionsDto {
            headers: Some(vec![("X-Test".to_string(), "1".to_string())]),
            ..Default::default()
        }),
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

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let done = state
                .engine
                .store()
                .get_job(odm_core::JobId(id))
                .is_ok_and(|job| job.state() == odm_core::JobState::Completed);
            if done {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("download should complete");

    assert!(
        seen.lock().unwrap().iter().any(|value| value == "1"),
        "origin never saw x-test: 1"
    );
}

/// A server that sends a 200-byte file at 20 bytes a second.
async fn slow_file_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/slow.bin",
        get(|| async {
            let chunks = futures_util::stream::unfold(0_u32, |sent| async move {
                if sent == 10 {
                    return None;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from_static(&[b'z'; 20])),
                    sent + 1,
                ))
            });
            (
                [(CONTENT_LENGTH, "200")],
                axum::body::Body::from_stream(chunks),
            )
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/slow.bin")
}

#[tokio::test]
async fn shutdown_pauses_running_jobs() {
    let url = slow_file_server().await;
    let dir = tempdir().unwrap();
    let engine = Engine::new(Store::open(dir.path().join("odm.sqlite")).unwrap()).unwrap();
    let state = AppState::new(engine.clone(), DaemonConfig::for_test(dir.path(), "secret"));
    let job = engine
        .add(
            &url,
            dir.path().to_path_buf(),
            odm_core::JobOptions::default(),
        )
        .unwrap();
    state.spawn_download(job.id(), "default", 3);
    for _ in 0..200 {
        if engine.store().get_job(job.id()).unwrap().state() == odm_core::JobState::Downloading {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(odm_daemon::server::serve(listener, state, async move {
        let _ = stopped.await;
    }));
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), server)
        .await
        .expect("the server did not shut down")
        .unwrap()
        .unwrap();

    assert_eq!(
        engine.store().get_job(job.id()).unwrap().state(),
        odm_core::JobState::Paused
    );
}

/// Adds a job and walks it to `state` through the allowed transitions.
fn job_in_state(
    engine: &Engine,
    dir: &std::path::Path,
    state: odm_core::JobState,
) -> odm_core::Job {
    use odm_core::JobState;
    let mut job = engine
        .add(
            "http://127.0.0.1:9/never.bin",
            dir.to_path_buf(),
            odm_core::JobOptions::default(),
        )
        .unwrap();
    let path: &[JobState] = match state {
        JobState::Queued => &[],
        JobState::Downloading => &[JobState::Probing, JobState::Downloading],
        JobState::Completed => &[
            JobState::Probing,
            JobState::Downloading,
            JobState::Completed,
        ],
        other => panic!("unsupported state {other:?}"),
    };
    for step in path {
        job.transition_to(*step).unwrap();
    }
    engine.store().update_job(&job).unwrap();
    job
}

async fn post_to(app: &Router, uri: &str) -> StatusCode {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

fn app_with_engine(dir: &std::path::Path) -> (Router, Engine) {
    let engine = Engine::new(Store::open(dir.join("odm.sqlite")).unwrap()).unwrap();
    let state = AppState::new(engine.clone(), DaemonConfig::for_test(dir, "secret"));
    (build_router_with_state(state), engine)
}

#[tokio::test]
async fn cancel_of_queued_job_sends_state_event() {
    let dir = tempdir().unwrap();
    let (app, engine) = app_with_engine(dir.path());
    let job = job_in_state(&engine, dir.path(), odm_core::JobState::Queued);
    let mut events = engine.subscribe();

    let status = post_to(&app, &format!("/api/v1/downloads/{}/cancel", job.id().0)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        events.try_recv().unwrap(),
        odm_core::Event::JobStateChanged {
            id: job.id(),
            state: odm_core::JobState::Cancelled,
        }
    );
}

#[tokio::test]
async fn resume_of_completed_job_is_409() {
    let dir = tempdir().unwrap();
    let (app, engine) = app_with_engine(dir.path());
    let job = job_in_state(&engine, dir.path(), odm_core::JobState::Completed);

    let status = post_to(&app, &format!("/api/v1/downloads/{}/resume", job.id().0)).await;

    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn retry_of_running_job_is_409() {
    let dir = tempdir().unwrap();
    let (app, engine) = app_with_engine(dir.path());
    let job = job_in_state(&engine, dir.path(), odm_core::JobState::Downloading);

    let status = post_to(&app, &format!("/api/v1/downloads/{}/retry", job.id().0)).await;

    assert_eq!(status, StatusCode::CONFLICT);
}
