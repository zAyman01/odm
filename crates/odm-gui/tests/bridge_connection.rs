//! Daemon bridge integration tests with a local mock daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::ws::{Message as AxumWsMessage, WebSocketUpgrade};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::StreamExt;
use odm_api::{
    DownloadItemDto, Event, GrabberRequest, GrabberResponse, HealthResponse, ListDownloadsResponse,
    ListQueuesResponse, QueueDto, UpdateQueueRequest, WsAuthRequest, WsAuthResponse,
};
use odm_gui::bridge::{DaemonBridge, run_bridge};
use odm_gui::messages::{GuiEvent, UserAction};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

fn health_route() -> axum::routing::MethodRouter {
    get(|| async {
        Json(HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
        })
    })
}

async fn serve_mock(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn empty_grab() -> Json<GrabberResponse> {
    Json(GrabberResponse {
        files: Vec::new(),
        pages_crawled: 0,
    })
}

fn grab_action() -> UserAction {
    UserAction::StartGrab(Box::new(GrabberRequest {
        url: "http://127.0.0.1/".to_string(),
        depth: None,
        extensions: None,
        pattern: None,
        respect_robots: None,
        max_pages: None,
    }))
}

async fn next_grab_result(
    ui_rx: &mut mpsc::UnboundedReceiver<GuiEvent>,
) -> Result<GrabberResponse, String> {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(120), ui_rx.recv())
            .await
            .unwrap()
            .unwrap();
        if let GuiEvent::GrabberResult(result) = event {
            return result;
        }
    }
}

#[tokio::test]
async fn editing_schedule_keeps_max_concurrent() {
    let patched: Arc<Mutex<Option<UpdateQueueRequest>>> = Arc::new(Mutex::new(None));
    let sink = patched.clone();
    let app = Router::new()
        .route("/api/v1/health", health_route())
        .route(
            "/api/v1/queues",
            get(|| async {
                Json(ListQueuesResponse {
                    queues: vec![QueueDto {
                        id: 2,
                        name: "music".to_string(),
                        max_concurrent: 7,
                        schedule_start: None,
                        schedule_stop: None,
                        schedule_days: None,
                    }],
                })
            }),
        )
        .route(
            "/api/v1/queues/{name}",
            axum::routing::patch(move |Json(body): Json<UpdateQueueRequest>| {
                let sink = sink.clone();
                async move {
                    *sink.lock().unwrap() = Some(body);
                    axum::http::StatusCode::OK
                }
            }),
        );
    let server_url = serve_mock(app).await;

    let (ui_tx, _ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    let _bridge = DaemonBridge::start(server_url, None, ui_tx, action_rx);
    action_tx
        .send(UserAction::UpdateQueueSchedule {
            name: "music".to_string(),
            start_time: "22:00".to_string(),
            stop_time: Some("06:00".to_string()),
            days: None,
        })
        .unwrap();

    let mut body = None;
    for _ in 0..50 {
        body = patched.lock().unwrap().clone();
        if body.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let body = body.expect("the schedule was never sent");
    assert_eq!(body.max_concurrent, 7);
    assert_eq!(body.schedule_start.as_deref(), Some("22:00"));
}

// Real time: with paused time the runtime can jump past a timeout while the socket is still pending.
#[tokio::test]
async fn grab_request_is_not_killed_by_default_timeout() {
    let app = Router::new().route("/api/v1/health", health_route()).route(
        "/api/v1/grabber",
        axum::routing::post(|| async {
            tokio::time::sleep(Duration::from_secs(11)).await;
            empty_grab()
        }),
    );
    let server_url = serve_mock(app).await;

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    tokio::spawn(run_bridge(server_url, None, ui_tx, action_rx));
    action_tx.send(grab_action()).unwrap();

    let result = next_grab_result(&mut ui_rx).await;

    assert!(result.is_ok(), "grab failed: {result:?}");
}

#[tokio::test]
async fn actions_run_concurrently() {
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let grab_log = log.clone();
    let pause_log = log.clone();
    let app = Router::new()
        .route("/api/v1/health", health_route())
        .route(
            "/api/v1/grabber",
            axum::routing::post(move || {
                let log = grab_log.clone();
                async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    log.lock().unwrap().push("grab_done");
                    empty_grab()
                }
            }),
        )
        .route(
            "/api/v1/downloads/{id}/pause",
            axum::routing::post(move || {
                let log = pause_log.clone();
                async move {
                    log.lock().unwrap().push("pause");
                    axum::http::StatusCode::OK
                }
            }),
        );
    let server_url = serve_mock(app).await;

    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    tokio::spawn(run_bridge(server_url, None, ui_tx, action_rx));
    action_tx.send(grab_action()).unwrap();
    action_tx.send(UserAction::Pause(1)).unwrap();

    next_grab_result(&mut ui_rx).await.unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["pause", "grab_done"]);
}

#[tokio::test]
async fn ws_reconnect_triggers_full_refresh() {
    let downloads_gets = Arc::new(AtomicUsize::new(0));
    let ws_connections = Arc::new(AtomicUsize::new(0));
    let gets = downloads_gets.clone();
    let connections = ws_connections.clone();
    let app = Router::new()
        .route("/api/v1/health", health_route())
        .route(
            "/api/v1/downloads",
            get(move || {
                let gets = gets.clone();
                async move {
                    gets.fetch_add(1, Ordering::SeqCst);
                    Json(ListDownloadsResponse {
                        downloads: Vec::new(),
                    })
                }
            }),
        )
        .route(
            "/api/v1/events",
            get(move |ws: WebSocketUpgrade| {
                let connections = connections.clone();
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        connections.fetch_add(1, Ordering::SeqCst);
                        if socket.next().await.is_some() {
                            let resp = serde_json::to_string(&WsAuthResponse {
                                authenticated: true,
                            })
                            .unwrap();
                            let _ = socket.send(AxumWsMessage::Text(resp.into())).await;
                        }
                        // Dropping the socket restarts the stream, as a daemon restart would.
                    })
                }
            }),
        );
    let server_url = serve_mock(app).await;

    let (ui_tx, _ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (_action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    let _bridge = DaemonBridge::start(server_url, Some("t".to_string()), ui_tx, action_rx);

    for _ in 0..80 {
        if ws_connections.load(Ordering::SeqCst) >= 2 && downloads_gets.load(Ordering::SeqCst) >= 3
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "no refresh after reconnect: {} ws connections, {} GET /downloads",
        ws_connections.load(Ordering::SeqCst),
        downloads_gets.load(Ordering::SeqCst)
    );
}
