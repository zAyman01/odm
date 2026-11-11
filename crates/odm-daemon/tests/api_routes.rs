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
