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

#[test]
fn origin_allowed_accepts_own_port_and_extensions_only() {
    use odm_daemon::auth::origin_allowed;
    for origin in [
        "http://127.0.0.1:6820",
        "http://localhost:6820",
        "http://[::1]:6820",
        "chrome-extension://abcdef",
        "moz-extension://1234-5678",
    ] {
        assert!(origin_allowed(origin, 6820), "{origin} should be allowed");
    }
    for origin in [
        "http://127.0.0.1:9999",
        "http://localhost:3000",
        "http://evil.example",
        "http://127.0.0.1:6820.evil.example",
        "https://127.0.0.1:6820",
        "null",
    ] {
        assert!(!origin_allowed(origin, 6820), "{origin} should be rejected");
    }
}

#[test]
fn origin_allowed_with_ephemeral_port_accepts_any_loopback_port() {
    use odm_daemon::auth::origin_allowed;
    assert!(origin_allowed("http://127.0.0.1:41234", 0));
    assert!(!origin_allowed("http://evil.example:41234", 0));
}

async fn preflight(origin: &str) -> Option<String> {
    let dir = tempdir().unwrap();
    let mut config = DaemonConfig::for_test(dir.path(), "t");
    config.bind_addr = "127.0.0.1:6820".to_string();
    let app = build_router(config).unwrap();
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .header(header::ORIGIN, origin)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    resp.headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

#[tokio::test]
async fn cors_rejects_other_localhost_port() {
    assert_eq!(preflight("http://127.0.0.1:9999").await, None);
}

#[tokio::test]
async fn cors_allows_same_port_and_extensions() {
    assert_eq!(
        preflight("http://127.0.0.1:6820").await.as_deref(),
        Some("http://127.0.0.1:6820")
    );
    assert_eq!(
        preflight("chrome-extension://abcdef").await.as_deref(),
        Some("chrome-extension://abcdef")
    );
}

mod allowed_hosts {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use odm_core::{Engine, Store};
    use odm_daemon::config::DaemonConfig;
    use odm_daemon::server::build_router_with_state;
    use odm_daemon::state::AppState;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn app_allowing(hosts: Option<&str>) -> Router {
        let dir = tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        if let Some(hosts) = hosts {
            store.set_setting("allowed_hosts", hosts).unwrap();
        }
        let engine = Engine::new(store).unwrap();
        let state = AppState::new(engine, DaemonConfig::for_test(dir.path(), "secret"));
        build_router_with_state(state)
    }

    async fn health_status(app: &Router, host: &str) -> StatusCode {
        let req = Request::builder()
            .uri("/api/v1/health")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn host_in_allow_list_passes() {
        let app = app_allowing(Some("nas.lan, Media.Local"));

        assert_eq!(health_status(&app, "nas.lan:6820").await, StatusCode::OK);
        assert_eq!(health_status(&app, "media.local").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn host_not_in_list_is_403() {
        let app = app_allowing(Some("nas.lan"));

        assert_eq!(
            health_status(&app, "attacker.com:6820").await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn loopback_always_passes() {
        let app = app_allowing(Some("nas.lan"));

        for host in ["127.0.0.1:6820", "localhost", "[::1]:6820"] {
            assert_eq!(health_status(&app, host).await, StatusCode::OK, "{host}");
        }
    }

    #[tokio::test]
    async fn changing_the_allow_list_needs_no_restart() {
        let app = app_allowing(None);
        assert_eq!(health_status(&app, "nas.lan").await, StatusCode::FORBIDDEN);

        let patch = Request::builder()
            .method("PATCH")
            .uri("/api/v1/settings")
            .header(header::HOST, "127.0.0.1")
            .header(header::AUTHORIZATION, "Bearer secret")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"allowed_hosts":"nas.lan"}"#))
            .unwrap();
        let resp = app.clone().oneshot(patch).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        assert_eq!(health_status(&app, "nas.lan").await, StatusCode::OK);
    }
}
