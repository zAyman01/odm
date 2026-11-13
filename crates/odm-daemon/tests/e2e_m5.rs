//! End-to-end integration tests for Milestone 5 features in odm-daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode};
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, ChecksumAlgorithm, ChecksumDto, CreateQueueRequest,
    DownloadDetailDto, JobOptionsDto, ListQueuesResponse, UpdateQueueRequest,
};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router_with_state;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn e2e_m5_checksums_and_mirrors_persisted() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    let req_body = AddDownloadRequest {
        url: "http://primary.example.com/test.iso".to_string(),
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: Some("test.iso".to_string()),
        queue: Some("default".to_string()),
        category: None,
        options: Some(JobOptionsDto {
            segments: Some(4),
            speed_limit: None,
            headers: None,
            user_agent: None,
            referer: None,
            cookies: None,
            checksum: Some(ChecksumDto {
                algorithm: ChecksumAlgorithm::Sha256,
                digest: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                    .to_string(),
            }),
            mirrors: Some(vec![
                "http://mirror1.example.com/test.iso".to_string(),
                "http://mirror2.example.com/test.iso".to_string(),
            ]),
            ..Default::default()
        }),
        backend: None,
    };

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&req_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&body).unwrap();

    let detail_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();

    let detail_resp = app.clone().oneshot(detail_req).await.unwrap();
    assert_eq!(detail_resp.status(), StatusCode::OK);
    let detail_body = axum::body::to_bytes(detail_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&detail_body).unwrap();

    let cs = detail.options.checksum.unwrap();
    assert_eq!(cs.algorithm, ChecksumAlgorithm::Sha256);
    assert_eq!(
        cs.digest,
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    );

    let mirrors = detail.options.mirrors.unwrap();
    assert_eq!(mirrors.len(), 2);
    assert_eq!(mirrors[0], "http://mirror1.example.com/test.iso");
    assert_eq!(mirrors[1], "http://mirror2.example.com/test.iso");
}

#[tokio::test]
async fn e2e_m5_category_auto_routing_on_download() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    let req_body = AddDownloadRequest {
        url: "http://example.com/dist.tar.gz".to_string(),
        target_dir: None,
        filename: None,
        queue: Some("default".to_string()),
        category: None,
        options: None,
        backend: None,
    };

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&req_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&body).unwrap();

    let detail_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();

    let detail_resp = app.clone().oneshot(detail_req).await.unwrap();
    assert_eq!(detail_resp.status(), StatusCode::OK);
    let detail_body = axum::body::to_bytes(detail_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&detail_body).unwrap();

    assert_eq!(detail.item.category.as_deref(), Some("Compressed"));
}

#[tokio::test]
async fn e2e_m5_queue_scheduling_crud() {
    let dir = tempdir().unwrap();
    let store = Store::open(dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_router_with_state(state);

    // 1. Create scheduled queue
    let create_body = CreateQueueRequest {
        name: "nightly".to_string(),
        max_concurrent: 3,
        schedule_start: Some("01:00".to_string()),
        schedule_stop: Some("05:00".to_string()),
        schedule_days: Some(vec![
            "mon".to_string(),
            "wed".to_string(),
            "fri".to_string(),
        ]),
    };

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/queues")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&create_body).unwrap()))
        .unwrap();

    let create_resp = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);

    // 2. Verify queue in list
    let list_req = Request::builder()
        .method("GET")
        .uri("/api/v1/queues")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();

    let list_resp = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: ListQueuesResponse = serde_json::from_slice(&list_body).unwrap();
    let q = list.queues.iter().find(|q| q.name == "nightly").unwrap();
    assert_eq!(q.schedule_start.as_deref(), Some("01:00"));
    assert_eq!(q.schedule_stop.as_deref(), Some("05:00"));
    assert_eq!(
        q.schedule_days.as_ref().unwrap(),
        &vec!["mon".to_string(), "wed".to_string(), "fri".to_string()]
    );

    // 3. Update queue schedule
    let update_body = UpdateQueueRequest {
        max_concurrent: 4,
        schedule_start: Some("02:30".to_string()),
        schedule_stop: Some("06:30".to_string()),
        schedule_days: Some(vec!["sat".to_string(), "sun".to_string()]),
    };

    let update_req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/queues/nightly")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&update_body).unwrap()))
        .unwrap();

    let update_resp = app.clone().oneshot(update_req).await.unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);

    // 4. Verify updated schedule
    let list_req2 = Request::builder()
        .method("GET")
        .uri("/api/v1/queues")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(Body::empty())
        .unwrap();
    let list_resp2 = app.clone().oneshot(list_req2).await.unwrap();
    let list_body2 = axum::body::to_bytes(list_resp2.into_body(), usize::MAX)
        .await
        .unwrap();
    let list2: ListQueuesResponse = serde_json::from_slice(&list_body2).unwrap();
    let q2 = list2.queues.iter().find(|q| q.name == "nightly").unwrap();
    assert_eq!(q2.schedule_start.as_deref(), Some("02:30"));
    assert_eq!(q2.schedule_stop.as_deref(), Some("06:30"));
    assert_eq!(
        q2.schedule_days.as_ref().unwrap(),
        &vec!["sat".to_string(), "sun".to_string()]
    );
}
