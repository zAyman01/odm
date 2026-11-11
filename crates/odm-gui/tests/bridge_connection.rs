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
                        error: None,
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
