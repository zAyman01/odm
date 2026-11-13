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
    let app = build_router(config).unwrap();

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
    let app = build_router(config).unwrap();

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
    let app = build_router(config).unwrap();

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
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .header(header::AUTHORIZATION, "Bearer test-secret-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

fn load_config(dir: &std::path::Path) -> DaemonConfig {
    DaemonConfig::load_or_init(
        Some(dir.join("config")),
        Some(dir.join("data")),
        Some("127.0.0.1:0".to_string()),
    )
    .unwrap()
}

#[test]
fn generated_token_is_64_hex_and_unique() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();

    let a = load_config(first.path()).token;
    let b = load_config(second.path()).token;

    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "token was {a}");
    assert_ne!(a, b);
    let on_disk = std::fs::read_to_string(first.path().join("config").join("token")).unwrap();
    assert_eq!(on_disk.trim(), a);
    // A second start keeps the stored token.
    assert_eq!(load_config(first.path()).token, a);
}

#[test]
fn empty_token_file_is_regenerated() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(dir.path().join("config").join("token"), "  \n").unwrap();

    let token = load_config(dir.path()).token;

    assert_eq!(token.len(), 64);
}

#[test]
fn empty_expected_token_never_matches() {
    assert!(!odm_daemon::auth::token_matches("", ""));
    assert!(!odm_daemon::auth::token_matches("", "anything"));
    assert!(odm_daemon::auth::token_matches("abc", "abc"));
    assert!(!odm_daemon::auth::token_matches("abc", "abd"));
}

#[tokio::test]
async fn empty_configured_token_does_not_accept_empty_bearer() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .header(header::AUTHORIZATION, "Bearer ")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
