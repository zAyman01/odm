#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use odm_plugin::{
    PluginError, PluginHost, PluginManifest, PluginRole, ResolvedDownload, ResolverRequest,
    ResolverResponse,
};

static TEST_DIR: OnceLock<PathBuf> = OnceLock::new();

fn fake_binary_name() -> &'static str {
    if cfg!(windows) {
        "fake_plugin.exe"
    } else {
        "fake_plugin"
    }
}

fn get_test_dir() -> &'static Path {
    TEST_DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("odm_test_plugin_host_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let source_path = dir.join("fake_plugin_source.rs");
        let out_binary = dir.join(fake_binary_name());

        let code = r##"
use std::io::{self, Read};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("success");

    match mode {
        "timeout" => {
            std::thread::sleep(std::time::Duration::from_millis(2000));
        }
        "crash" => {
            eprintln!("simulated plugin crash error message");
            std::process::exit(1);
        }
        "malformed" => {
            println!("this is not json at all");
        }
        "empty" => {
            // output nothing
        }
        "empty_downloads" => {
            println!("{}", r#"{"downloads": []}"#);
        }
        "success" => {
            let mut input = String::new();
            let _ = io::stdin().read_to_string(&mut input);
            println!(
                "{}",
                r#"{"downloads": [{"url": "https://example.com/resolved/stream.mp4", "headers": {"Authorization": "Bearer 123"}, "filename": "video.mp4"}]}"#
            );
        }
        other => {
            eprintln!("unknown mode: {other}");
            std::process::exit(2);
        }
    }
}
"##;
        std::fs::write(&source_path, code).expect("failed to write fake_plugin source");
        let status = std::process::Command::new("rustc")
            .arg(&source_path)
            .arg("-o")
            .arg(&out_binary)
            .status()
            .expect("failed to compile fake_plugin");
        assert!(status.success(), "rustc compilation of fake_plugin failed");
        dir
    })
}

fn create_manifest(mode_arg: &str) -> PluginManifest {
    PluginManifest::new(
        "test-plugin",
        "1.0.0",
        Some("Test Plugin Host".to_string()),
        PluginRole::Resolver,
        vec!["*://example.com/*".to_string()],
        vec![fake_binary_name().to_string(), mode_arg.to_string()],
    )
    .unwrap()
}

#[tokio::test]
async fn test_resolve_success() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("success");

    let response = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_secs(5),
    )
    .await
    .unwrap();

    assert_eq!(response.downloads.len(), 1);
    let dl = &response.downloads[0];
    assert_eq!(dl.url, "https://example.com/resolved/stream.mp4");
    assert_eq!(
        dl.headers.get("Authorization"),
        Some(&"Bearer 123".to_string())
    );
    assert_eq!(dl.filename.as_deref(), Some("video.mp4"));
}

#[tokio::test]
async fn test_resolve_timeout() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("timeout");

    let result = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_millis(150),
    )
    .await;

    match result {
        Err(PluginError::Timeout(msg)) => {
            assert!(
                msg.contains("timeout") || msg.contains("timed out") || msg.contains("150ms"),
                "unexpected timeout msg: {msg}"
            );
        }
        other => panic!("expected PluginError::Timeout, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_crash() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("crash");

    let result = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_secs(5),
    )
    .await;

    match result {
        Err(PluginError::ProcessFailed(msg)) => {
            assert!(
                msg.contains("simulated plugin crash error message"),
                "expected stderr message in: {msg}"
            );
        }
        other => panic!("expected PluginError::ProcessFailed, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_malformed_json() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("malformed");

    let result = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_secs(5),
    )
    .await;

    match result {
        Err(PluginError::InvalidJson(msg)) => {
            assert!(!msg.is_empty());
        }
        other => panic!("expected PluginError::InvalidJson, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_empty_output() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("empty");

    let result = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_secs(5),
    )
    .await;

    match result {
        Err(PluginError::InvalidJson(msg)) => {
            assert!(!msg.is_empty());
        }
        other => panic!("expected PluginError::InvalidJson, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_empty_downloads_list() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("empty_downloads");

    let result = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/watch?v=123",
        Duration::from_secs(5),
    )
    .await;

    match result {
        Err(PluginError::InvalidJson(msg)) => {
            assert!(
                msg.contains("empty downloads list"),
                "expected empty downloads list message, got: {msg}"
            );
        }
        other => panic!("expected PluginError::InvalidJson, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_resolve_default_timeout() {
    let test_dir = get_test_dir();
    let manifest = create_manifest("success");

    let response =
        PluginHost::resolve_default(&manifest, test_dir, "https://example.com/watch?v=123")
            .await
            .unwrap();

    assert_eq!(response.downloads.len(), 1);
}

#[cfg(windows)]
#[tokio::test]
async fn test_resolve_windows_batch_script() {
    let test_dir = get_test_dir();
    let batch_file = test_dir.join("test_resolver.cmd");
    let batch_content = "@echo off\r\necho {\"downloads\": [{\"url\": \"https://example.com/batch.mp4\", \"headers\": {}, \"filename\": \"batch.mp4\"}]}\r\n";
    std::fs::write(&batch_file, batch_content).expect("failed to write batch file");

    let manifest = PluginManifest::new(
        "batch-plugin",
        "1.0.0",
        None,
        PluginRole::Resolver,
        vec!["*://example.com/*".to_string()],
        vec!["test_resolver.cmd".to_string()],
    )
    .unwrap();

    let response = PluginHost::resolve(
        &manifest,
        test_dir,
        "https://example.com/video",
        Duration::from_secs(5),
    )
    .await
    .unwrap();

    assert_eq!(response.downloads.len(), 1);
    assert_eq!(response.downloads[0].url, "https://example.com/batch.mp4");
    assert_eq!(response.downloads[0].filename.as_deref(), Some("batch.mp4"));
}

#[test]
fn test_protocol_types_serde() {
    let req = ResolverRequest::new("https://example.com/video");
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("https://example.com/video"));
    assert!(json.contains("\"version\":\"1\"") || json.contains("\"version\": \"1\""));

    let json_resp = r#"{"downloads":[{"url":"https://example.com/file.mp4"}]}"#;
    let resp: ResolverResponse = serde_json::from_str(json_resp).unwrap();
    assert_eq!(resp.downloads.len(), 1);
    assert_eq!(resp.downloads[0].url, "https://example.com/file.mp4");
    assert!(resp.downloads[0].headers.is_empty());
    assert_eq!(resp.downloads[0].filename, None);

    let mut dl = ResolvedDownload::new("https://example.com/file.mp4");
    dl.headers
        .insert("User-Agent".to_string(), "odm/1.0".to_string());
    dl.filename = Some("file.mp4".to_string());
    let resp2 = ResolverResponse::new(vec![dl]);
    let json2 = serde_json::to_string(&resp2).unwrap();
    assert!(json2.contains("file.mp4"));
    assert!(json2.contains("User-Agent"));
}
