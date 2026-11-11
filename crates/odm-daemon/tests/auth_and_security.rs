//! Daemon security middleware: bearer auth, host validation and CORS.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use odm_api::ErrorResponse;
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_bypasses_auth() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let app = build_router(config);

    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn unauthenticated_request_is_rejected_with_401() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let app = build_router(config);

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "unauthorized");
}

#[tokio::test]
async fn invalid_host_header_is_rejected_with_403() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let app = build_router(config);

    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "attacker.com")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "invalid_host");
}

#[tokio::test]
async fn authenticated_request_with_valid_bearer_succeeds() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let app = build_router(config);

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .header(header::AUTHORIZATION, "Bearer test-secret-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
