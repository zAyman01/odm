#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::net::SocketAddr;
use std::process::{Child, Command};
use std::time::Duration;

use axum::Router;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use odm_api::{GrabberResponse, ListDownloadsResponse, PluginListResponse};
use tempfile::tempdir;
use tokio::net::TcpListener;

struct DaemonProcess(Child);

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct TestHttpServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn start_test_http_server() -> TestHttpServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new()
        .route("/robots.txt", get(robots_txt))
        .route("/index.html", get(index_html))
        .route("/image1.png", get(dummy_asset))
        .route("/file1.zip", get(dummy_asset));

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    TestHttpServer { addr, handle }
}

async fn robots_txt() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain")],
        "User-agent: *\nAllow: /\n",
    )
        .into_response()
}

async fn index_html() -> Response {
    let body = r#"<!DOCTYPE html>
<html>
<head><title>Test Grabber Page</title></head>
<body>
    <a href="/file1.zip">Download Zip</a>
    <img src="/image1.png" alt="Test Image">
</body>
</html>"#;
    ([(header::CONTENT_TYPE, "text/html")], body).into_response()
}

async fn dummy_asset() -> Response {
    (
        [(header::CONTENT_TYPE, "application/octet-stream")],
        "test payload",
    )
        .into_response()
}

async fn spawn_test_daemon() -> (DaemonProcess, String, String) {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();

    // Create a mock plugin in <data_dir>/plugins/test_plugin
    let plugin_dir = data_dir.join("plugins").join("test_plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    let manifest = r#"[plugin]
name = "test-plugin"
version = "1.0.0"
description = "A test plugin"
role = "resolver"
url_patterns = ["^https?://example\\.com/.*$"]
command = ["test-bin", "run"]
"#;
    fs::write(plugin_dir.join("plugin.toml"), manifest).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let odm_bin = std::path::PathBuf::from(env!("CARGO_BIN_EXE_odm"));
    let daemon_bin = odm_bin.with_file_name(if cfg!(windows) {
        "odm-daemon.exe"
    } else {
        "odm-daemon"
    });
    if !daemon_bin.exists() {
        let status = Command::new("cargo")
            .args(["build", "--bin", "odm-daemon"])
            .status()
            .unwrap();
        assert!(status.success());
    }

    let child = Command::new(daemon_bin)
        .args([
            "--bind",
            &addr.to_string(),
            "--config-dir",
            config_dir.to_str().unwrap(),
            "--data-dir",
            data_dir.to_str().unwrap(),
        ])
        .spawn()
        .unwrap();

    let proc = DaemonProcess(child);
    let server_url = format!("http://{addr}");

    let client = reqwest::Client::new();
    let health_url = format!("{server_url}/health");
    let mut online = false;
    for _ in 0..60 {
        if let Ok(resp) = client.get(&health_url).send().await
            && resp.status().is_success()
        {
            online = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(online, "Daemon did not come online in time");

    let token_path = config_dir.join("token");
    let token = fs::read_to_string(&token_path)
        .unwrap_or_default()
        .trim()
        .to_string();

    (proc, server_url, token)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cli_grab_depth_1_displays_found_files() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;
    let web_server = start_test_http_server().await;
    let url = format!("http://{}/index.html", web_server.addr);

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "grab",
            &url,
            "--depth",
            "1",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("image1.png"),
        "expected image1.png in stdout: {stdout}"
    );
    assert!(
        stdout.contains("file1.zip"),
        "expected file1.zip in stdout: {stdout}"
    );
    assert!(stdout.contains("URL") && stdout.contains("FILENAME"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cli_grab_add_enqueues_found_files() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;
    let web_server = start_test_http_server().await;
    let url = format!("http://{}/index.html", web_server.addr);

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "grab",
            &url,
            "--depth",
            "1",
            "--add",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Added download #"),
        "expected Added download # confirmation: {stdout}"
    );

    // Verify downloads are in the queue
    let list_out = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args(["--server", &server_url, "--token", &token, "list", "--json"])
        .output()
        .unwrap();
    assert!(list_out.status.success());
    let list_json: ListDownloadsResponse =
        serde_json::from_slice(&list_out.stdout).expect("valid downloads list json");
    assert_eq!(list_json.downloads.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cli_grab_json_prints_valid_json() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;
    let web_server = start_test_http_server().await;
    let url = format!("http://{}/index.html", web_server.addr);

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "grab",
            &url,
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resp: GrabberResponse =
        serde_json::from_slice(&output.stdout).expect("valid GrabberResponse json");
    assert_eq!(resp.files.len(), 2);
    let filenames: Vec<&str> = resp.files.iter().map(|f| f.filename.as_str()).collect();
    assert!(filenames.contains(&"image1.png"));
    assert!(filenames.contains(&"file1.zip"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cli_plugin_list_displays_warning_and_plugins() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args(["--server", &server_url, "--token", &token, "plugin", "list"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "Security notice: Plugins run with full user permissions without sandboxing."
        ),
        "expected security notice in output: {stdout}"
    );
    assert!(
        stdout.contains("test-plugin"),
        "expected test-plugin in output: {stdout}"
    );
    assert!(
        stdout.contains("1.0.0"),
        "expected version in output: {stdout}"
    );
    assert!(
        stdout.contains("resolver"),
        "expected role in output: {stdout}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_cli_plugin_list_json_outputs_valid_json() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "plugin",
            "list",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resp: PluginListResponse =
        serde_json::from_slice(&output.stdout).expect("valid PluginListResponse json");
    assert_eq!(resp.plugins.len(), 1);
    assert_eq!(resp.plugins[0].name, "test-plugin");
    assert_eq!(resp.plugins[0].version, "1.0.0");
    assert_eq!(resp.plugins[0].role, "resolver");
}
