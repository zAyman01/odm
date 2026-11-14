#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::Body;
use axum::http::header::{ACCEPT_RANGES, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, ErrorResponse, PluginListResponse,
    ProbeRequest, ProbeResponse,
};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_router;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

fn fake_binary_name() -> &'static str {
    if cfg!(windows) {
        "mock_plugin.exe"
    } else {
        "mock_plugin"
    }
}

fn compile_mock_plugin(dir: &Path) -> PathBuf {
    let source_path = dir.join("mock_plugin_source.rs");
    let out_binary = dir.join(fake_binary_name());

    let code = r##"
use std::env;
use std::io::{self, Read};

fn main() {
    let args: Vec<String> = env::args().collect();
    let target_url = args.get(1).map(|s| s.as_str()).unwrap_or("http://127.0.0.1:8080/file.bin");
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    println!(
        "{}",
        format!(r#"{{"downloads": [{{"url": "{}", "headers": {{}}, "filename": "resolved_file.bin"}}]}}"#, target_url)
    );
}
"##;

    std::fs::write(&source_path, code).unwrap();
    let status = std::process::Command::new("rustc")
        .arg(&source_path)
        .arg("-o")
        .arg(&out_binary)
        .status()
        .unwrap();
    assert!(status.success(), "rustc compilation failed");
    out_binary
}

struct TestServer {
    addr: SocketAddr,
}

impl TestServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let app = Router::new().route(
            "/file.bin",
            get(|| async {
                (
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (ACCEPT_RANGES, "bytes"),
                        (CONTENT_LENGTH, "1024"),
                    ],
                    vec![0u8; 1024],
                )
                    .into_response()
            })
            .head(|| async {
                (
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (ACCEPT_RANGES, "bytes"),
                        (CONTENT_LENGTH, "1024"),
                    ],
                    "",
                )
                    .into_response()
            }),
        );

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { addr }
    }
}

fn setup_test_env(target_url: &str) -> (tempfile::TempDir, DaemonConfig) {
    let temp_dir = tempdir().unwrap();
    let config_dir = temp_dir.path().join("config");
    let data_dir = temp_dir.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let plugin_dir = data_dir.join("plugins").join("sample");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    compile_mock_plugin(&plugin_dir);

    let manifest_toml = format!(
        r#"[plugin]
name = "sample-resolver"
version = "0.1.0"
description = "A sample resolver plugin"
role = "resolver"
url_patterns = ["^https?://(www\\.)?sample-service\\.com/.*$"]
command = ["{}", "{}"]
"#,
        fake_binary_name(),
        target_url
    );
    std::fs::write(plugin_dir.join("plugin.toml"), manifest_toml).unwrap();

    let config = DaemonConfig {
        config_dir,
        data_dir,
        bind_addr: "127.0.0.1:6820".to_string(),
        token: "test-token".to_string(),
    };

    (temp_dir, config)
}

#[tokio::test]
async fn plugins_list_unauthenticated_returns_401() {
    let (_dir, config) = setup_test_env("http://127.0.0.1:8080/file.bin");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/plugins")
        .header(HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "unauthorized");
}

#[tokio::test]
async fn plugins_list_authenticated_returns_sample_plugin() {
    let (_dir, config) = setup_test_env("http://127.0.0.1:8080/file.bin");
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/plugins")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let list: PluginListResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.plugins.len(), 1);
    let plugin = &list.plugins[0];
    assert_eq!(plugin.name, "sample-resolver");
    assert_eq!(plugin.version, "0.1.0");
    assert_eq!(
        plugin.description.as_deref(),
        Some("A sample resolver plugin")
    );
    assert_eq!(plugin.role, "resolver");
    assert_eq!(
        plugin.url_patterns,
        vec!["^https?://(www\\.)?sample-service\\.com/.*$"]
    );
}

#[tokio::test]
async fn probe_with_matching_url_selects_plugin_backend() {
    let server = TestServer::start().await;
    let target_url = format!("http://127.0.0.1:{}/file.bin", server.addr.port());
    let (_dir, config) = setup_test_env(&target_url);
    let app = build_router(config).unwrap();

    let probe_req = ProbeRequest {
        url: "https://sample-service.com/video/42".to_string(),
        user_agent: None,
    };
    let body_bytes = serde_json::to_vec(&probe_req).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/probe")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let probe_resp: ProbeResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(probe_resp.backend, "plugin");
    assert_eq!(probe_resp.filename, "resolved_file.bin");
    assert_eq!(probe_resp.size, Some(1024));
    assert!(probe_resp.ranges);
}

#[tokio::test]
async fn add_download_with_matching_url_selects_plugin_backend() {
    let server = TestServer::start().await;
    let target_url = format!("http://127.0.0.1:{}/file.bin", server.addr.port());
    let (_dir, config) = setup_test_env(&target_url);
    let app = build_router(config).unwrap();

    let add_req = AddDownloadRequest {
        url: "https://sample-service.com/media/999".to_string(),
        target_dir: None,
        filename: None,
        queue: None,
        category: None,
        options: None,
        backend: None,
    };
    let body_bytes = serde_json::to_vec(&add_req).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body_bytes))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();

    let req = Request::builder()
        .uri(format!("/api/v1/downloads/{}", add_resp.id))
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let detail: DownloadDetailDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(detail.backend.as_deref(), Some("plugin"));
}

#[tokio::test]
async fn bad_plugin_toml_does_not_disable_others() {
    let (dir, config) = setup_test_env("http://127.0.0.1:8080/file.bin");
    let broken = dir.path().join("data").join("plugins").join("a-broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.toml"), "this is [not valid toml").unwrap();
    let app = build_router(config).unwrap();

    let req = Request::builder()
        .uri("/api/v1/plugins")
        .header(HOST, "127.0.0.1:6820")
        .header(AUTHORIZATION, "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let list: PluginListResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list.plugins.len(), 1);
    assert_eq!(list.plugins[0].name, "sample-resolver");
}
