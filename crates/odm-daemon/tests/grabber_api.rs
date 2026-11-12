#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_api::{ErrorResponse, GrabberRequest, GrabberResponse};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

struct TestServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new()
            .route("/robots.txt", get(robots_txt))
            .route("/index.html", get(index_html))
            .route("/page2.html", get(page2_html))
            .route("/image1.png", get(dummy_asset))
            .route("/docs/report.pdf", get(dummy_asset))
            .route("/secret/file.zip", get(dummy_asset))
            .route("/image2.png", get(dummy_asset))
            .route("/archive.zip", get(dummy_asset));

        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { addr, handle }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain")],
        "User-agent: *\nDisallow: /secret/\n",
    )
        .into_response()
}

async fn index_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Index Page</title></head>
<body>
    <a href="/page2.html">Page 2</a>
    <a href="/secret/file.zip">Secret Zip</a>
    <img src="/image1.png" alt="Image 1">
    <a href="/docs/report.pdf">Report</a>
    <a href="https://external.example.com/ext.zip">External Link</a>
</body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn page2_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Page 2</title></head>
<body>
    <img src="/image2.png" alt="Image 2">
    <a href="/archive.zip">Archive</a>
</body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn dummy_asset() -> Response {
    (
        [(header::CONTENT_TYPE, "application/octet-stream")],
        "dummy content",
    )
        .into_response()
}

fn setup_test_daemon() -> (tempfile::TempDir, DaemonConfig) {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("config");
    let data_dir = temp_dir.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let config = DaemonConfig {
        config_dir,
        data_dir,
        bind_addr: "127.0.0.1:6820".to_string(),
        token: "test-token".to_string(),
    };

    (temp_dir, config)
}

#[tokio::test]
async fn grabber_unauthenticated_returns_401() {
    let (_dir, config) = setup_test_daemon();
    let app = build_router(config).unwrap();

    let payload = GrabberRequest {
        url: "http://127.0.0.1:8080/index.html".to_string(),
        depth: None,
        extensions: None,
        pattern: None,
        respect_robots: None,
        max_pages: None,
    };
    let body_json = serde_json::to_vec(&payload).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/grabber")
        .header(HOST, "127.0.0.1:6820")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_json))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "unauthorized");
}

#[tokio::test]
async fn grabber_invalid_url_returns_400() {
    let (_dir, config) = setup_test_daemon();
    let app = build_router(config).unwrap();

    let payload = GrabberRequest {
        url: "not-a-url".to_string(),
        depth: None,
        extensions: None,
        pattern: None,
        respect_robots: None,
        max_pages: None,
    };
    let body_json = serde_json::to_vec(&payload).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/grabber")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_json))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "bad_request");
    assert!(
        err.error.message.contains("invalid start url"),
        "expected error message to contain 'invalid start url', got: {}",
        err.error.message
    );
}

#[tokio::test]
async fn grabber_valid_start_url_returns_200_and_grab_response() {
    let server = TestServer::start().await;
    let (_dir, config) = setup_test_daemon();
    let app = build_router(config).unwrap();

    let payload = GrabberRequest {
        url: server.url("/index.html"),
        depth: Some(1),
        extensions: None,
        pattern: None,
        respect_robots: Some(true),
        max_pages: Some(10),
    };
    let body_json = serde_json::to_vec(&payload).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/grabber")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_json))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let grab_resp: GrabberResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(grab_resp.pages_crawled, 1);

    let filenames: Vec<&str> = grab_resp
        .files
        .iter()
        .map(|f| f.filename.as_str())
        .collect();
    assert!(filenames.contains(&"image1.png"));
    assert!(filenames.contains(&"report.pdf"));
    // Disallowed by robots.txt
    assert!(!filenames.contains(&"file.zip"));
    // External link not included
    assert!(!filenames.contains(&"ext.zip"));
}

#[tokio::test]
async fn grabber_extension_filter_returns_only_matching_files() {
    let server = TestServer::start().await;
    let (_dir, config) = setup_test_daemon();
    let app = build_router(config).unwrap();

    let payload = GrabberRequest {
        url: server.url("/index.html"),
        depth: Some(1),
        extensions: Some(vec!["png".to_string()]),
        pattern: None,
        respect_robots: Some(true),
        max_pages: Some(10),
    };
    let body_json = serde_json::to_vec(&payload).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/grabber")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_json))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let grab_resp: GrabberResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(grab_resp.pages_crawled, 1);

    let filenames: Vec<&str> = grab_resp
        .files
        .iter()
        .map(|f| f.filename.as_str())
        .collect();
    assert_eq!(filenames, vec!["image1.png"]);
}

#[tokio::test]
async fn grabber_respect_robots_false_includes_disallowed_paths() {
    let server = TestServer::start().await;
    let (_dir, config) = setup_test_daemon();
    let app = build_router(config).unwrap();

    let payload = GrabberRequest {
        url: server.url("/index.html"),
        depth: Some(1),
        extensions: None,
        pattern: None,
        respect_robots: Some(false),
        max_pages: Some(10),
    };
    let body_json = serde_json::to_vec(&payload).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/grabber")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_json))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let grab_resp: GrabberResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(grab_resp.pages_crawled, 1);

    let filenames: Vec<&str> = grab_resp
        .files
        .iter()
        .map(|f| f.filename.as_str())
        .collect();
    assert!(filenames.contains(&"image1.png"));
    assert!(filenames.contains(&"report.pdf"));
    assert!(filenames.contains(&"file.zip"));
}
