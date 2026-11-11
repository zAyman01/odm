//! End-to-end integration test: `odm add <url>` downloads a real file via daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;
use tempfile::tempdir;
use tokio::net::TcpListener;

struct DaemonProcess(Child);

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_test_file_server(data: Vec<u8>) -> (String, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/largefile.bin",
        get(move |req: axum::extract::Request| {
            let data = data.clone();
            async move {
                let range = req
                    .headers()
                    .get(axum::http::header::RANGE)
                    .and_then(|val| val.to_str().ok())
                    .and_then(|val| val.strip_prefix("bytes="))
                    .and_then(|val| val.split_once('-'))
                    .and_then(|(s, e)| Some((s.parse::<usize>().ok()?, e.parse::<usize>().ok()?)));

                if let Some((start, end)) = range {
                    let end = end.min(data.len().saturating_sub(1));
                    let slice = data[start..=end].to_vec();
                    (
                        StatusCode::PARTIAL_CONTENT,
                        [
                            (CONTENT_TYPE, "application/octet-stream"),
                            (
                                axum::http::header::CONTENT_RANGE,
                                format!("bytes {start}-{end}/{}", data.len()).leak() as &str,
                            ),
                            (CONTENT_LENGTH, slice.len().to_string().leak() as &str),
                            (ACCEPT_RANGES, "bytes"),
                        ],
                        slice,
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::OK,
                        [
                            (CONTENT_TYPE, "application/octet-stream"),
                            (CONTENT_LENGTH, data.len().to_string().leak() as &str),
                            (ACCEPT_RANGES, "bytes"),
                        ],
                        data,
                    )
                        .into_response()
                }
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}/largefile.bin"), addr)
}

#[tokio::test]
async fn odm_add_downloads_real_file_end_to_end() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let download_dir = temp.path().join("downloads");
    fs::create_dir_all(&download_dir).unwrap();

    // 1. Generate 256 KiB random test payload
    let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
    let (url, _) = start_test_file_server(payload.clone()).await;

    // 2. Pick a random free port for the daemon
    let daemon_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let daemon_addr = daemon_listener.local_addr().unwrap();
    drop(daemon_listener);

    // 3. Spawn odm-daemon
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
            &daemon_addr.to_string(),
            "--config-dir",
            config_dir.to_str().unwrap(),
            "--data-dir",
            data_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let _guard = DaemonProcess(child);

    // 4. Poll health endpoint until daemon is responsive
    let client = reqwest::Client::new();
    let health_url = format!("http://{daemon_addr}/api/v1/health");
    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&health_url).send().await
            && resp.status().is_success()
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "odm-daemon failed to start within timeout");

    // 5. Read generated token
    let token = fs::read_to_string(config_dir.join("token")).unwrap();
    let token = token.trim();

    // 6. Run `odm add <url>` via CLI
    let add_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &format!("http://{daemon_addr}"),
            "--token",
            token,
            "add",
            &url,
            "--dir",
            download_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        add_output.status.success(),
        "odm add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    // 7. Wait for file to finish downloading
    let downloaded_file = download_dir.join("largefile.bin");
    let mut finished = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if downloaded_file.exists()
            && !download_dir.join("largefile.bin.part").exists()
            && let Ok(meta) = fs::metadata(&downloaded_file)
            && meta.len() == payload.len() as u64
        {
            finished = true;
            break;
        }
    }

    assert!(finished, "download did not complete in time");

    // 8. Verify binary contents match exactly byte-for-byte
    let actual_bytes = fs::read(&downloaded_file).unwrap();
    assert_eq!(actual_bytes, payload);
}
