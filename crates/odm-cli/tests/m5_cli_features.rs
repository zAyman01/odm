//! CLI tests for M5 features: batch expansion, checksums, mirrors, metalink, categories, queue schedules.
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

    // Wait for health endpoint
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

    // Read generated token from config file
    let token_path = config_dir.join("token");
    let token = fs::read_to_string(&token_path)
        .unwrap_or_default()
        .trim()
        .to_string();

    (proc, server_url, token)
}

#[tokio::test]
async fn cli_add_expands_batch_bracket_patterns() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    // Run odm add with batch pattern: http://127.0.0.1:9/file[1-3].bin
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "add",
            "http://127.0.0.1:9/file[1-3].bin",
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
        stdout.contains("file1.bin")
            || stdout.contains("file[1-3].bin")
            || stdout.contains("\"id\"")
    );

    // Verify 3 downloads were created via odm list --json
    let list_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args(["--server", &server_url, "--token", &token, "--json", "list"])
        .output()
        .unwrap();

    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("file1.bin"));
    assert!(list_stdout.contains("file2.bin"));
    assert!(list_stdout.contains("file3.bin"));
}

#[tokio::test]
async fn cli_add_with_checksum_and_mirrors() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "add",
            "http://127.0.0.1:9/archive.zip",
            "--checksum",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "--mirror",
            "http://127.0.0.1:9/mirror1.zip",
            "--mirror",
            "http://127.0.0.1:9/mirror2.zip",
            "--category",
            "Compressed",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify category and options via odm info
    let info_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "info",
            "1",
        ])
        .output()
        .unwrap();

    assert!(info_output.status.success());
    let info_stdout = String::from_utf8_lossy(&info_output.stdout);
    assert!(
        info_stdout.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}

#[tokio::test]
async fn cli_category_management_commands() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    // 1. odm category list
    let list_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "category",
            "list",
        ])
        .output()
        .unwrap();

    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("Compressed"));

    // 2. odm category set
    let set_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "category",
            "set",
            "TestCat",
            "--dir",
            "/test/cat",
            "--exts",
            "tst,xyz",
        ])
        .output()
        .unwrap();

    assert!(set_output.status.success());
    let set_stdout = String::from_utf8_lossy(&set_output.stdout);
    assert!(set_stdout.contains("TestCat"));

    // 3. odm category delete
    let del_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "category",
            "delete",
            "TestCat",
        ])
        .output()
        .unwrap();

    assert!(del_output.status.success());
}

#[tokio::test]
async fn cli_queue_schedule_command() {
    let (_daemon, server_url, token) = spawn_test_daemon().await;

    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &server_url,
            "--token",
            &token,
            "--json",
            "queue",
            "schedule",
            "default",
            "--start",
            "23:00",
            "--stop",
            "06:00",
            "--days",
            "mon,tue,wed",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("23:00"));
}

#[test]
fn mirrors_with_multiple_urls_is_an_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "add",
            "http://127.0.0.1:1/a.bin",
            "http://127.0.0.1:1/b.bin",
            "--mirror",
            "http://127.0.0.1:1/mirror.bin",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("single URL"), "stderr: {stderr}");
}

#[test]
fn json_output_escapes_the_server_url() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            "http://127.0.0.1:1\\",
            "--json",
            "daemon",
            "status",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap();
    let value: serde_json::Value =
        serde_json::from_str(first_line).unwrap_or_else(|e| panic!("not JSON ({e}): {first_line}"));
    assert_eq!(value["server"], "http://127.0.0.1:1\\");
}
