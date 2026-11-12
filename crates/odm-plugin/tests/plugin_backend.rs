#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;
use odm_core::backend::{
    Backend, BackendError, ControlToken, JobContext, ProgressSink, RunOutcome,
};
use odm_core::job::{BackendKind, Job, JobOptions, Source};
use odm_core::limiter::{Limiter, TokenBucket};
use odm_core::store::Store;
use odm_plugin::{PluginBackend, PluginRegistry};
use tempfile::TempDir;
use tokio::net::TcpListener;

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
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("resolve");

    match mode {
        "resolve" => {
            let mut input = String::new();
            let _ = io::stdin().read_to_string(&mut input);
            let target_url = args.get(2).map(|s| s.as_str()).unwrap_or("http://127.0.0.1:8080/file.bin");
            println!(
                "{}",
                format!(r#"{{"downloads": [{{"url": "{}", "headers": {{"X-Mock-Plugin": "active"}}, "filename": "resolved_file.bin"}}]}}"#, target_url)
            );
        }
        "fail" => {
            eprintln!("simulated resolver failure");
            std::process::exit(1);
        }
        _ => {
            std::process::exit(2);
        }
    }
}
"##;

    std::fs::write(&source_path, code).expect("failed to write mock plugin source");
    let status = std::process::Command::new("rustc")
        .arg(&source_path)
        .arg("-o")
        .arg(&out_binary)
        .status()
        .expect("failed to compile mock plugin with rustc");
    assert!(status.success(), "rustc compilation failed");
    out_binary
}

struct TestServer {
    addr: SocketAddr,
    file_bytes: Vec<u8>,
}

impl TestServer {
    async fn start(content: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bytes_to_serve = content.clone();

        let app = Router::new()
            .route(
                "/file.bin",
                get(move || {
                    let b = bytes_to_serve.clone();
                    async move {
                        (
                            [
                                (CONTENT_TYPE, "application/octet-stream"),
                                (ACCEPT_RANGES, "bytes"),
                                (CONTENT_LENGTH, &b.len().to_string()),
                            ],
                            b,
                        )
                            .into_response()
                    }
                }),
            )
            .route(
                "/fail.bin",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR.into_response() }),
            );

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            file_bytes: content,
        }
    }

    fn file_url(&self) -> String {
        format!("http://{}/file.bin", self.addr)
    }
}

fn setup_test_plugins(root_dir: &Path, binary_path: &Path, server_file_url: &str) -> PathBuf {
    let plugins_dir = root_dir.join("plugins");
    std::fs::create_dir_all(&plugins_dir).unwrap();

    // 1. Success resolver plugin
    let success_dir = plugins_dir.join("success_plugin");
    std::fs::create_dir_all(&success_dir).unwrap();
    let bin_dest = success_dir.join(fake_binary_name());
    std::fs::copy(binary_path, &bin_dest).unwrap();

    let manifest_toml = format!(
        r#"[plugin]
name = "mock-resolver"
version = "1.0.0"
description = "Mock Resolver Plugin"
role = "resolver"
url_patterns = ["*://example.com/*", "*://test-plugin.local/*"]
command = ["{}", "resolve", "{}"]
"#,
        fake_binary_name(),
        server_file_url
    );
    std::fs::write(success_dir.join("plugin.toml"), manifest_toml).unwrap();

    // 2. Failing resolver plugin
    let fail_dir = plugins_dir.join("fail_plugin");
    std::fs::create_dir_all(&fail_dir).unwrap();
    let fail_bin_dest = fail_dir.join(fake_binary_name());
    std::fs::copy(binary_path, &fail_bin_dest).unwrap();

    let fail_manifest_toml = format!(
        r#"[plugin]
name = "failing-resolver"
version = "1.0.0"
description = "Failing Resolver Plugin"
role = "resolver"
url_patterns = ["*://fail.example.com/*"]
command = ["{}", "fail"]
"#,
        fake_binary_name()
    );
    std::fs::write(fail_dir.join("plugin.toml"), fail_manifest_toml).unwrap();

    plugins_dir
}

#[tokio::test]
async fn test_plugin_registry_discover() {
    let temp_dir = TempDir::new().unwrap();
    let bin_path = compile_mock_plugin(temp_dir.path());
    let plugins_dir = setup_test_plugins(temp_dir.path(), &bin_path, "http://127.0.0.1:9/file.bin");

    let mut registry = PluginRegistry::new(plugins_dir);
    let count = registry.discover().unwrap();
    assert_eq!(count, 2, "must discover 2 valid plugins");

    // Matches test URLs
    assert!(
        registry
            .find_for_url("https://example.com/video/123")
            .is_some()
    );
    assert!(
        registry
            .find_for_url("http://test-plugin.local/item")
            .is_some()
    );
    assert!(
        registry
            .find_for_url("https://fail.example.com/item")
            .is_some()
    );
    assert!(
        registry
            .find_for_url("https://unrelated.org/file")
            .is_none()
    );

    // list() exposes PluginInfo
    let list = registry.list();
    assert_eq!(list.len(), 2);
    let mock = list.iter().find(|p| p.name == "mock-resolver").unwrap();
    assert_eq!(mock.version, "1.0.0");
    assert_eq!(mock.role, "resolver");
    assert_eq!(
        mock.url_patterns,
        vec!["*://example.com/*", "*://test-plugin.local/*"]
    );
}

#[tokio::test]
async fn test_plugin_backend_probe() {
    let payload = b"Hello from odm plugin backend!".to_vec();
    let server = TestServer::start(payload).await;

    let temp_dir = TempDir::new().unwrap();
    let bin_path = compile_mock_plugin(temp_dir.path());
    let plugins_dir = setup_test_plugins(temp_dir.path(), &bin_path, &server.file_url());

    let mut registry = PluginRegistry::new(plugins_dir);
    registry.discover().unwrap();
    let backend = PluginBackend::new(Arc::new(tokio::sync::RwLock::new(registry)));

    assert_eq!(backend.kind(), BackendKind::Plugin);

    let source = Source::Url("https://example.com/item/100".to_string());
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    assert_eq!(probe.backend, BackendKind::Plugin);
    assert_eq!(probe.filename, "resolved_file.bin");
    assert_eq!(probe.total_bytes, Some(server.file_bytes.len() as u64));
    assert!(probe.supports_ranges);
}

#[tokio::test]
async fn test_plugin_backend_run_complete() {
    let payload = b"Payload content for testing plugin download run execution.".to_vec();
    let server = TestServer::start(payload.clone()).await;

    let temp_dir = TempDir::new().unwrap();
    let bin_path = compile_mock_plugin(temp_dir.path());
    let plugins_dir = setup_test_plugins(temp_dir.path(), &bin_path, &server.file_url());

    let mut registry = PluginRegistry::new(plugins_dir);
    registry.discover().unwrap();
    let backend = PluginBackend::new(Arc::new(tokio::sync::RwLock::new(registry)));

    let source = Source::Url("https://example.com/item/100".to_string());
    let probe = backend
        .probe(&source, &JobOptions::default())
        .await
        .unwrap();

    let download_dir = temp_dir.path().join("downloads");
    std::fs::create_dir_all(&download_dir).unwrap();

    let mut job = Job::new(source, download_dir, JobOptions::default());
    job.apply_probe(probe);

    let store = Store::open_in_memory().unwrap();
    store.insert_job(&mut job).unwrap();

    let (_control_handle, control) = ControlToken::new();
    let (progress, mut progress_rx) = ProgressSink::new();
    let limiter = Limiter::new(Arc::new(TokenBucket::new(0)), None);

    let ctx = JobContext {
        job: job.clone(),
        control,
        progress,
        store: store.clone(),
        limiter,
    };

    let outcome = backend.run(ctx).await.unwrap();
    assert_eq!(outcome, RunOutcome::Completed);

    let target_path = job.target_path();
    assert!(
        target_path.exists(),
        "target downloaded file must exist at {}",
        target_path.display()
    );
    let content = std::fs::read(&target_path).unwrap();
    assert_eq!(
        content, payload,
        "downloaded content must match source payload"
    );

    // Verify progress was reported
    let mut last_progress = None;
    while let Ok(p) = progress_rx.try_recv() {
        last_progress = Some(p);
    }
    assert!(
        last_progress.is_some(),
        "at least one progress update must have been reported"
    );
    assert_eq!(
        last_progress.unwrap().downloaded_bytes,
        payload.len() as u64
    );
}

#[tokio::test]
async fn test_plugin_backend_probe_unsupported() {
    let temp_dir = TempDir::new().unwrap();
    let bin_path = compile_mock_plugin(temp_dir.path());
    let plugins_dir = setup_test_plugins(temp_dir.path(), &bin_path, "http://127.0.0.1:9/file.bin");

    let mut registry = PluginRegistry::new(plugins_dir);
    registry.discover().unwrap();
    let backend = PluginBackend::new(Arc::new(tokio::sync::RwLock::new(registry)));

    let source = Source::Url("https://not-a-plugin-pattern.org/file.zip".to_string());
    let err = backend.probe(&source, &JobOptions::default()).await;

    match err {
        Err(BackendError::Unsupported) => {}
        other => panic!("expected BackendError::Unsupported, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_plugin_backend_probe_plugin_error() {
    let temp_dir = TempDir::new().unwrap();
    let bin_path = compile_mock_plugin(temp_dir.path());
    let plugins_dir = setup_test_plugins(temp_dir.path(), &bin_path, "http://127.0.0.1:9/file.bin");

    let mut registry = PluginRegistry::new(plugins_dir);
    registry.discover().unwrap();
    let backend = PluginBackend::new(Arc::new(tokio::sync::RwLock::new(registry)));

    let source = Source::Url("https://fail.example.com/video/999".to_string());
    let err = backend.probe(&source, &JobOptions::default()).await;

    match err {
        Err(BackendError::Plugin(msg)) => {
            assert!(
                msg.contains("simulated resolver failure")
                    || msg.contains("exited with")
                    || msg.contains("failed"),
                "error message must describe plugin failure, got: {msg}"
            );
        }
        other => panic!("expected BackendError::Plugin, got: {other:?}"),
    }
}
