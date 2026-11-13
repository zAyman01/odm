//! Integration tests for embedded Web UI static file serving.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use odm_api::HealthResponse;
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn webui_root_serves_index_html() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-token");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/html"),
        "expected text/html, got {content_type}"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), 100_000)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&bytes);
    assert!(
        body_str.contains("<title>odm</title>"),
        "expected <title>odm</title> in body"
    );
}

#[tokio::test]
async fn webui_spa_fallback_for_unknown_path() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-token");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/nonexistent-path")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/html"),
        "expected text/html, got {content_type}"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), 100_000)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&bytes);
    assert!(
        body_str.contains("<title>odm</title>"),
        "expected <title>odm</title> in fallback body"
    );
}

#[tokio::test]
async fn api_routes_not_intercepted_by_webui() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-token");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("application/json"),
        "expected application/json, got {content_type}"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let health: HealthResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health.status, "ok");
}

#[tokio::test]
async fn api_protected_routes_still_require_auth() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-token");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
