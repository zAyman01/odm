//! CLI tests for M6 torrent backend: magnet links, .torrent files, file selection, and seed ratio options.
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
async fn cli_add_magnet_url() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let magnet =
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=test_cli_magnet.bin";
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            magnet,
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
    assert_eq!(detail.backend.as_deref(), Some("torrent"));
}

#[tokio::test]
async fn cli_add_torrent_file() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let dir = tempdir().unwrap();
    let torrent_path = dir.path().join("sample.torrent");
    fs::write(&torrent_path, b"d8:announce0:e").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            "--torrent-file",
            torrent_path.to_str().unwrap(),
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
    assert_eq!(detail.backend.as_deref(), Some("torrent"));
}

#[tokio::test]
async fn cli_add_with_select_files_and_seed_ratio() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=test_options.bin";
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            magnet,
            "--select-files",
            "0,2",
            "--seed-ratio",
            "0.5",
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
    assert_eq!(detail.backend.as_deref(), Some("torrent"));
    assert_eq!(detail.options.selected_files, Some(vec![0, 2]));
    assert_eq!(detail.options.seed_ratio, Some(0.5));
}

#[tokio::test]
async fn torrent_file_path_is_sent_absolute() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("relative.torrent"), b"d8:announce0:e").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .current_dir(dir.path())
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "add",
            "--torrent-file",
            "relative.torrent",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let added: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    let id = added["id"].as_i64().unwrap();

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
    assert!(info.status.success());
    let detail: odm_api::DownloadDetailDto =
        serde_json::from_str(&String::from_utf8_lossy(&info.stdout)).unwrap();
    // macOS reaches the temp folder through a symlink (/var -> /private/var), so compare real paths.
    let sent = std::path::PathBuf::from(&detail.item.url);
    assert!(sent.is_absolute());
    assert_eq!(
        sent.canonicalize().unwrap(),
        dir.path().join("relative.torrent").canonicalize().unwrap()
    );
}
