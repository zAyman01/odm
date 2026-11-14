//! End-to-end GUI flow test: simulates complete user interaction flow through the ViewState.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use odm_api::{
    CreateQueueRequest, DownloadDetailDto, DownloadItemDto, Event, HealthResponse, JobOptionsDto,
    ListQueuesResponse, ProbeResponse, SegmentDto, SettingsDto,
};
use odm_gui::bridge::run_bridge;
use odm_gui::messages::{GuiEvent, UserAction};
use odm_gui::state::{Filter, ViewState};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[test]
fn full_gui_lifecycle_flow() {
    let mut state = ViewState::default();

    // 1. User copies URL, clipboard emits URL captured event
    state.apply_event(GuiEvent::UrlCaptured(
        "http://127.0.0.1:8080/sample.tar.gz".to_string(),
    ));
    assert!(state.add_dialog.open);
    assert_eq!(state.add_dialog.url, "http://127.0.0.1:8080/sample.tar.gz");

    // 2. Probe returns metadata
    state.apply_event(GuiEvent::ProbeResult(Ok(ProbeResponse {
        backend: "http".to_string(),
        filename: "sample.tar.gz".to_string(),
        size: Some(10_000_000),
        ranges: true,
        etag: Some("w/123".to_string()),
        last_modified: None,
        torrent_files: None,
        qualities: None,
    })));
    assert_eq!(state.add_dialog.filename, "sample.tar.gz");

    // 3. User submits download
    state.close_add_dialog();

    // 4. Daemon confirms download added
    let item = DownloadItemDto {
        id: 42,
        url: "http://127.0.0.1:8080/sample.tar.gz".to_string(),
        filename: "sample.tar.gz".to_string(),
        state: "downloading".to_string(),
        total_bytes: Some(10_000_000),
        downloaded_bytes: 0,
        progress_pct: 0.0,
        speed_bps: 0,
        queue: "default".to_string(),
        target_dir: "C:\\Downloads".to_string(),
        category: None,
        error: None,
        backend: None,
    };
    state.apply_event(GuiEvent::EngineEvent(Event::JobAdded(item)));
    assert_eq!(state.downloads.len(), 1);
    assert_eq!(state.downloads[0].id, 42);

    // 5. Select item and update progress
    state.select_download(Some(42));
    assert_eq!(state.selected_id, Some(42));

    state.apply_event(GuiEvent::EngineEvent(Event::JobProgress {
        id: 42,
        downloaded_bytes: 5_000_000,
        total_bytes: Some(10_000_000),
        speed_bps: 1_000_000,
    }));
    assert_eq!(state.downloads[0].downloaded_bytes, 5_000_000);
    assert_eq!(state.downloads[0].progress_pct, 50.0);

    // 6. Complete job
    state.apply_event(GuiEvent::EngineEvent(Event::JobStateChanged {
        id: 42,
        state: "completed".to_string(),
    }));
    assert_eq!(state.downloads[0].state, "completed");

    // 7. Verify filtering
    state.filter = Filter::Active;
    assert!(state.filtered_downloads().is_empty());
    state.filter = Filter::Completed;
    assert_eq!(state.filtered_downloads().len(), 1);

    // 8. Delete job
    state.apply_event(GuiEvent::EngineEvent(Event::JobRemoved(42)));
    assert!(state.downloads.is_empty());
    assert_eq!(state.selected_id, None);
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

fn start_bridge(
    server_url: String,
) -> (
    mpsc::UnboundedSender<UserAction>,
    mpsc::UnboundedReceiver<GuiEvent>,
) {
    let (ui_tx, ui_rx) = mpsc::unbounded_channel::<GuiEvent>();
    let (action_tx, action_rx) = mpsc::unbounded_channel::<UserAction>();
    tokio::spawn(run_bridge(
        server_url,
        Some("t".to_string()),
        ui_tx,
        action_rx,
    ));
    (action_tx, ui_rx)
}

fn item(id: i64) -> DownloadItemDto {
    DownloadItemDto {
        id,
        url: "http://127.0.0.1/a.bin".to_string(),
        filename: "a.bin".to_string(),
        state: "downloading".to_string(),
        total_bytes: Some(2048),
        downloaded_bytes: 1024,
        progress_pct: 50.0,
        speed_bps: 0,
        queue: "default".to_string(),
        target_dir: "/tmp".to_string(),
        category: None,
        error: None,
        backend: None,
    }
}

#[tokio::test]
async fn detail_panel_gets_segments_for_selected_job() {
    let app = Router::new().route("/api/v1/health", health_route()).route(
        "/api/v1/downloads/{id}",
        get(|Path(id): Path<i64>| async move {
            Json(DownloadDetailDto {
                item: item(id),
                segments: vec![
                    SegmentDto {
                        index: 0,
                        start: 0,
                        end: 1023,
                        downloaded: 1024,
                        is_complete: true,
                    },
                    SegmentDto {
                        index: 1,
                        start: 1024,
                        end: 2047,
                        downloaded: 0,
                        is_complete: false,
                    },
                ],
                options: JobOptionsDto::default(),
                backend: None,
            })
        }),
    );
    let (action_tx, mut ui_rx) = start_bridge(serve_mock(app).await);
    let mut state = ViewState::default();
    state.select_download(Some(1));

    action_tx.send(UserAction::FetchDetail(1)).unwrap();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), ui_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let is_detail = matches!(event, GuiEvent::DownloadDetail(_));
        state.apply_event(event);
        if is_detail {
            break;
        }
    }

    let detail = state.selected_detail.expect("no detail for the selection");
    assert_eq!(detail.segments.len(), 2);
}

#[tokio::test]
async fn cancel_retry_and_create_queue_actions_reach_daemon() {
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (cancel_log, retry_log, create_log, list_log) =
        (log.clone(), log.clone(), log.clone(), log.clone());
    let app = Router::new()
        .route("/api/v1/health", health_route())
        .route(
            "/api/v1/downloads/{id}/cancel",
            post(move |Path(id): Path<i64>| {
                let log = cancel_log.clone();
                async move {
                    log.lock().unwrap().push(format!("cancel:{id}"));
                    StatusCode::OK
                }
            }),
        )
        .route(
            "/api/v1/downloads/{id}/retry",
            post(move |Path(id): Path<i64>| {
                let log = retry_log.clone();
                async move {
                    log.lock().unwrap().push(format!("retry:{id}"));
                    StatusCode::OK
                }
            }),
        )
        .route(
            "/api/v1/queues",
            post(move |Json(body): Json<CreateQueueRequest>| {
                let log = create_log.clone();
                async move {
                    log.lock()
                        .unwrap()
                        .push(format!("create:{}:{}", body.name, body.max_concurrent));
                    StatusCode::OK
                }
            })
            .get(move || {
                let log = list_log.clone();
                async move {
                    log.lock().unwrap().push("list_queues".to_string());
                    Json(ListQueuesResponse { queues: Vec::new() })
                }
            }),
        );
    let (action_tx, _ui_rx) = start_bridge(serve_mock(app).await);

    action_tx.send(UserAction::Cancel(3)).unwrap();
    action_tx.send(UserAction::Retry(4)).unwrap();
    action_tx
        .send(UserAction::CreateQueue {
            name: "music".to_string(),
            concurrency: 2,
        })
        .unwrap();

    let mut seen = Vec::new();
    for _ in 0..50 {
        seen = log.lock().unwrap().clone();
        // One queue listing comes from the first sync; the second follows the new queue.
        if seen.len() >= 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![
            "cancel:3",
            "create:music:2",
            "list_queues",
            "list_queues",
            "retry:4"
        ]
    );
}

#[tokio::test]
async fn settings_are_loaded_on_connect() {
    let app = Router::new().route("/api/v1/health", health_route()).route(
        "/api/v1/settings",
        get(|| async {
            Json(SettingsDto {
                download_dir: Some("/data/dl".to_string()),
                default_segments: 6,
                global_speed_limit: 5000,
                default_retry_count: 3,
                seed_ratio: 1.0,
                torrent_listen_port: 0,
                ffmpeg_path: "/usr/bin/ffmpeg".to_string(),
                ytdlp_path: String::new(),
                allowed_hosts: String::new(),
            })
        }),
    );
    let (_action_tx, mut ui_rx) = start_bridge(serve_mock(app).await);
    let mut state = ViewState::default();

    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), ui_rx.recv())
            .await
            .expect("settings never arrived")
            .unwrap();
        let is_settings = matches!(event, GuiEvent::SettingsLoaded(_));
        state.apply_event(event);
        if is_settings {
            break;
        }
    }

    assert_eq!(state.settings_dialog.default_segments, 6);
    assert_eq!(state.settings_dialog.download_dir, "/data/dl");
    assert_eq!(state.settings_dialog.ffmpeg_path, "/usr/bin/ffmpeg");
}
