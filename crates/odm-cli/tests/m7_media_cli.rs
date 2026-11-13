//! CLI tests for M7 media support: HLS, DASH, and --quality flag.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::{Child, Command};
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;

struct DaemonProcess(Child);

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn spawn_test_daemon() -> (DaemonProcess, String, String) {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();

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

#[tokio::test]
async fn cli_add_m3u8_url() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let url = "https://example.com/stream.m3u8";
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            url,
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let id = val["id"].as_i64().unwrap();

    let info_out = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "info",
            &id.to_string(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(info_out.status.success());
    let info_stdout = String::from_utf8_lossy(&info_out.stdout);
    let detail: odm_api::DownloadDetailDto = serde_json::from_str(&info_stdout).unwrap();
    assert_eq!(detail.backend.as_deref(), Some("hls"));
    assert_eq!(detail.item.backend.as_deref(), Some("hls"));
}

#[tokio::test]
async fn cli_add_m3u8_url_with_quality() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let url = "https://example.com/stream.m3u8";
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            url,
            "--quality",
            "720p",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let id = val["id"].as_i64().unwrap();

    let info_out = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "info",
            &id.to_string(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(info_out.status.success());
    let info_stdout = String::from_utf8_lossy(&info_out.stdout);
    let detail: odm_api::DownloadDetailDto = serde_json::from_str(&info_stdout).unwrap();
    assert_eq!(detail.backend.as_deref(), Some("hls"));
    assert_eq!(detail.options.quality.as_deref(), Some("720p"));
}

#[tokio::test]
async fn cli_add_mpd_url_with_quality() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let url = "https://example.com/manifest.mpd";
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            url,
            "--quality",
            "1080p",
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let id = val["id"].as_i64().unwrap();

    let info_out = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "info",
            &id.to_string(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(info_out.status.success());
    let info_stdout = String::from_utf8_lossy(&info_out.stdout);
    let detail: odm_api::DownloadDetailDto = serde_json::from_str(&info_stdout).unwrap();
    assert_eq!(detail.backend.as_deref(), Some("dash"));
    assert_eq!(detail.options.quality.as_deref(), Some("1080p"));
}

#[tokio::test]
async fn cli_add_ytdlp_flag_reports_missing_binary() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;
    // The daemon only reaches the yt-dlp check when no yt-dlp is installed;
    // otherwise the add succeeds and the job is a yt-dlp job.
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            "https://example.com/watch?v=1",
            "--ytdlp",
            "--json",
        ])
        .output()
        .unwrap();

    if output.status.success() {
        let val: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
        let id = val["id"].as_i64().unwrap();
        let info = Command::new(env!("CARGO_BIN_EXE_odm"))
            .args([
                "--server",
                &server_url,
                "--token",
                &token,
                "info",
                &id.to_string(),
                "--json",
            ])
            .output()
            .unwrap();
        let detail: odm_api::DownloadDetailDto = serde_json::from_slice(&info.stdout).unwrap();
        assert_eq!(detail.backend.as_deref(), Some("yt-dlp"));
    } else {
        // With --json the CLI reports errors on stdout.
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("yt-dlp"), "stdout: {stdout}");
    }
}
