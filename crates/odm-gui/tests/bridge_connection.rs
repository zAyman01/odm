//! Daemon bridge integration tests with a local mock daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::ws::{Message as AxumWsMessage, WebSocketUpgrade};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::StreamExt;
use odm_api::{
    DownloadItemDto, Event, HealthResponse, ListDownloadsResponse, WsAuthRequest, WsAuthResponse,
};
use odm_gui::bridge::DaemonBridge;
use odm_gui::messages::{GuiEvent, UserAction};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[tokio::test]
async fn bridge_connects_syncs_downloads_and_forwards_events() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new()
        .route(
            "/api/v1/health",
            get(|| async {
                Json(HealthResponse {
                    status: "ok".to_string(),
                    version: "0.1.0".to_string(),
                })
            }),
        )
        .route(
            "/api/v1/downloads",
            get(|| async {
                Json(ListDownloadsResponse {
                    downloads: vec![DownloadItemDto {
                        id: 1,
                        url: "http://127.0.0.1/test.bin".to_string(),
                        filename: "test.bin".to_string(),
                        state: "downloading".to_string(),
                        total_bytes: Some(1024),
                        downloaded_bytes: 512,
                        progress_pct: 50.0,
                        speed_bps: 100,
                        queue: "default".to_string(),
                        target_dir: "/tmp".to_string(),
                        category: None,
                        error: None,
                        backend: None,
                    }],
                })
            }),
        )
        .route(
            "/api/v1/events",
            get(|ws: WebSocketUpgrade| async move {
                ws.on_upgrade(|mut socket| async move {
                    if let Some(Ok(AxumWsMessage::Text(msg))) = socket.next().await
                        && let Ok(req) = serde_json::from_str::<WsAuthRequest>(&msg)
                        && req.token == "test-token"
                    {
                        let resp = serde_json::to_string(&WsAuthResponse {
                            authenticated: true,
                        })
                        .unwrap();
                        let _ = socket.send(AxumWsMessage::Text(resp.into())).await;

                        // Broadcast a job progress event
                        let event = Event::JobProgress {
                            id: 1,
                            downloaded_bytes: 1024,
                            total_bytes: Some(1024),
                            speed_bps: 200,
                        };
                        let evt_json = serde_json::to_string(&event).unwrap();
                        let _ = socket.send(AxumWsMessage::Text(evt_json.into())).await;
                    }
                })
            }),
        );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (_action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();

    let server_url = format!("http://{addr}");
    let _bridge = DaemonBridge::start(server_url, Some("test-token".to_string()), ui_tx, action_rx);

    // Verify initial connection and download list event received
    let mut got_connected = false;
    let mut got_downloads = false;
    let mut got_progress = false;

    for _ in 0..30 {
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(100), ui_rx.recv()).await {
            match event {
                Some(GuiEvent::DaemonStatus(true)) => got_connected = true,
                Some(GuiEvent::DownloadsList(list)) => {
                    if !list.is_empty() && list[0].id == 1 {
                        got_downloads = true;
                    }
                }
                Some(GuiEvent::EngineEvent(Event::JobProgress {
                    id,
                    downloaded_bytes,
                    ..
                })) if id == 1 && downloaded_bytes == 1024 => {
                    got_progress = true;
                }
                _ => {}
            }
            if got_connected && got_downloads && got_progress {
                break;
            }
        }
    }

    assert!(got_connected, "Bridge did not report daemon connection");
    assert!(got_downloads, "Bridge did not load initial downloads");
    assert!(
        got_progress,
        "Bridge did not forward websocket progress event"
    );
}

#[test]
fn bridge_starts_without_ambient_runtime() {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            addr_tx.send(listener.local_addr().unwrap()).unwrap();
            let app = Router::new().route(
                "/api/v1/health",
                get(|| async {
                    Json(HealthResponse {
                        status: "ok".to_string(),
                        version: "0.1.0".to_string(),
                    })
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });
    });
    let addr = addr_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (_action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    let _bridge = DaemonBridge::start(format!("http://{addr}"), None, ui_tx, action_rx);

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut connected = false;
    while std::time::Instant::now() < deadline {
        if let Ok(GuiEvent::DaemonStatus(true)) = ui_rx.try_recv() {
            connected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(connected, "bridge never reported a daemon connection");
}

#[test]
fn default_server_url_matches_daemon_port() {
    assert_eq!(
        odm_gui::bridge::server_url_from(None),
        "http://127.0.0.1:6820"
    );
    assert_eq!(
        odm_gui::bridge::server_url_from(Some("http://10.0.0.2:9".to_string())),
        "http://10.0.0.2:9"
    );
}

#[test]
fn read_token_file_ignores_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("token");
    std::fs::write(&path, "  \n").unwrap();
    assert_eq!(odm_gui::bridge::read_token_file(&path), None);
    std::fs::write(&path, "abc\n").unwrap();
    assert_eq!(
        odm_gui::bridge::read_token_file(&path).as_deref(),
        Some("abc")
    );
    assert_eq!(
        odm_gui::bridge::read_token_file(&dir.path().join("missing")),
        None
    );
}

#[test]
fn default_token_path_matches_daemon() {
    let expected =
        directories::ProjectDirs::from("", "", "odm").map(|dirs| dirs.config_dir().join("token"));
    assert_eq!(odm_gui::bridge::default_token_path(), expected);
}
