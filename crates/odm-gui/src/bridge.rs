//! Background Tokio bridge connecting the GUI to the odm-daemon REST & WebSocket API.
use std::fs;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use odm_api::{
    CreateQueueRequest, ErrorResponse, Event, ListCategoriesResponse, ListDownloadsResponse,
    ListQueuesResponse, ProbeRequest, ProbeResponse, SetCategoryRequest, UpdateQueueRequest,
    WsAuthRequest, WsAuthResponse,
};
use reqwest::Client;
use reqwest::header::AUTHORIZATION;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::messages::{GuiEvent, UserAction};

pub struct DaemonBridge {
    _server_url: String,
}

impl DaemonBridge {
    pub fn start(
        server_url: String,
        explicit_token: Option<String>,
        ui_tx: mpsc::UnboundedSender<GuiEvent>,
        mut action_rx: mpsc::UnboundedReceiver<UserAction>,
    ) -> Self {
        let server_url_clone = server_url.clone();

        // The GUI main thread has no Tokio runtime, so the bridge owns one on its own thread.
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => {
                    let _ = ui_tx.send(GuiEvent::DaemonStatus(false));
                    return;
                }
            };
            runtime.block_on(async move {
                let client = Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .unwrap_or_else(|_| Client::new());

                let token = explicit_token.or_else(|| {
                    directories::ProjectDirs::from("com", "odm", "odm")
                        .and_then(|dirs| fs::read_to_string(dirs.config_dir().join("token")).ok())
                        .map(|s| s.trim().to_string())
                });

                // 1. Health check & report connection status
                let health_url = format!("{server_url_clone}/api/v1/health");
                let mut connected = false;
                for _ in 0..10 {
                    if let Ok(resp) = client.get(&health_url).send().await
                        && resp.status().is_success()
                    {
                        connected = true;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                let _ = ui_tx.send(GuiEvent::DaemonStatus(connected));

                // 2. Initial fetch of downloads, categories, queues
                if connected {
                    let _ = fetch_and_send_downloads(
                        &client,
                        &server_url_clone,
                        token.as_deref(),
                        &ui_tx,
                    )
                    .await;
                    let _ = fetch_and_send_categories(
                        &client,
                        &server_url_clone,
                        token.as_deref(),
                        &ui_tx,
                    )
                    .await;
                    let _ =
                        fetch_and_send_queues(&client, &server_url_clone, token.as_deref(), &ui_tx)
                            .await;
                }

                // 3. Spawn WebSocket subscriber task
                let ws_server_url = server_url_clone.clone();
                let ws_token = token.clone();
                let ws_ui_tx = ui_tx.clone();

                tokio::spawn(async move {
                    let ws_url = if let Some(stripped) = ws_server_url.strip_prefix("https://") {
                        format!("wss://{stripped}/api/v1/events")
                    } else if let Some(stripped) = ws_server_url.strip_prefix("http://") {
                        format!("ws://{stripped}/api/v1/events")
                    } else {
                        format!("ws://{ws_server_url}/api/v1/events")
                    };

                    loop {
                        if let Ok((mut ws_stream, _)) = connect_async(&ws_url).await {
                            // First message: WebSocket authentication
                            let auth_body = WsAuthRequest {
                                token: ws_token.clone().unwrap_or_default(),
                            };
                            if let Ok(auth_json) = serde_json::to_string(&auth_body) {
                                let _ = ws_stream.send(WsMessage::Text(auth_json.into())).await;
                            }

                            // Read response
                            if let Some(Ok(WsMessage::Text(resp_text))) = ws_stream.next().await
                                && let Ok(auth_resp) =
                                    serde_json::from_str::<WsAuthResponse>(&resp_text)
                                && auth_resp.authenticated
                            {
                                while let Some(msg) = ws_stream.next().await {
                                    if let Ok(WsMessage::Text(text)) = msg
                                        && let Ok(evt) = serde_json::from_str::<Event>(&text)
                                    {
                                        let _ = ws_ui_tx.send(GuiEvent::EngineEvent(evt));
                                    }
                                }
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                });

                // 4. Main action handler loop
                while let Some(action) = action_rx.recv().await {
                    match action {
                        UserAction::AddDownload(req) => {
                            let url = format!("{server_url_clone}/api/v1/downloads");
                            let mut request = client.post(&url);
                            if let Some(ref tok) = token {
                                request = request.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            if let Ok(resp) = request.json(&req).send().await {
                                if resp.status().is_success() {
                                    let _ = fetch_and_send_downloads(
                                        &client,
                                        &server_url_clone,
                                        token.as_deref(),
                                        &ui_tx,
                                    )
                                    .await;
                                } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                    let _ = ui_tx.send(GuiEvent::ErrorMessage(err.error.message));
                                }
                            }
                        }

                        UserAction::Probe(target_url) => {
                            let url = format!("{server_url_clone}/api/v1/probe");
                            let mut request = client.post(&url);
                            if let Some(ref tok) = token {
                                request = request.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let body = ProbeRequest {
                                url: target_url,
                                user_agent: None,
                            };
                            match request.json(&body).send().await {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        if let Ok(probe) = resp.json::<ProbeResponse>().await {
                                            let _ = ui_tx.send(GuiEvent::ProbeResult(Ok(probe)));
                                        }
                                    } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                        let _ = ui_tx
                                            .send(GuiEvent::ProbeResult(Err(err.error.message)));
                                    } else {
                                        let _ = ui_tx.send(GuiEvent::ProbeResult(Err(
                                            "Probe failed".to_string(),
                                        )));
                                    }
                                }
                                Err(e) => {
                                    let _ = ui_tx.send(GuiEvent::ProbeResult(Err(format!(
                                        "Network error: {e}"
                                    ))));
                                }
                            }
                        }

                        UserAction::SelectQuality(_) => {
                            // Handled in local GUI state
                        }

                        UserAction::Pause(id) => {
                            let url = format!("{server_url_clone}/api/v1/downloads/{id}/pause");
                            let mut req = client.post(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let _ = req.send().await;
                        }

                        UserAction::Resume(id) => {
                            let url = format!("{server_url_clone}/api/v1/downloads/{id}/resume");
                            let mut req = client.post(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let _ = req.send().await;
                        }

                        UserAction::Cancel(id) => {
                            let url = format!("{server_url_clone}/api/v1/downloads/{id}/cancel");
                            let mut req = client.post(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let _ = req.send().await;
                        }

                        UserAction::Remove { id, delete_file } => {
                            let url = format!(
                                "{server_url_clone}/api/v1/downloads/{id}?delete_file={delete_file}"
                            );
                            let mut req = client.delete(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let _ = req.send().await;
                        }

                        UserAction::UpdateSettings(settings) => {
                            let url = format!("{server_url_clone}/api/v1/settings");
                            let mut req = client.patch(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let _ = req.json(&settings).send().await;
                        }

                        UserAction::CreateQueue { name, concurrency } => {
                            let url = format!("{server_url_clone}/api/v1/queues");
                            let mut req = client.post(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let body = CreateQueueRequest {
                                name,
                                max_concurrent: concurrency,
                                schedule_start: None,
                                schedule_stop: None,
                                schedule_days: None,
                            };
                            let _ = req.json(&body).send().await;
                        }

                        UserAction::SetCategory {
                            name,
                            directory,
                            extensions,
                        } => {
                            let url = format!("{server_url_clone}/api/v1/categories/{name}");
                            let mut req = client.put(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let body = SetCategoryRequest {
                                directory,
                                extensions,
                            };
                            if let Ok(resp) = req.json(&body).send().await {
                                if resp.status().is_success() {
                                    let _ = fetch_and_send_categories(
                                        &client,
                                        &server_url_clone,
                                        token.as_deref(),
                                        &ui_tx,
                                    )
                                    .await;
                                } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                    let _ = ui_tx.send(GuiEvent::ErrorMessage(err.error.message));
                                }
                            }
                        }

                        UserAction::UpdateQueueSchedule {
                            name,
                            start_time,
                            stop_time,
                            days,
                        } => {
                            let url = format!("{server_url_clone}/api/v1/queues/{name}");
                            let mut req = client.patch(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            let body = UpdateQueueRequest {
                                max_concurrent: 4,
                                schedule_start: Some(start_time),
                                schedule_stop: stop_time,
                                schedule_days: days,
                            };
                            if let Ok(resp) = req.json(&body).send().await {
                                if resp.status().is_success() {
                                    let _ = fetch_and_send_queues(
                                        &client,
                                        &server_url_clone,
                                        token.as_deref(),
                                        &ui_tx,
                                    )
                                    .await;
                                } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                    let _ = ui_tx.send(GuiEvent::ErrorMessage(err.error.message));
                                }
                            }
                        }

                        UserAction::StartGrab(grab_req) => {
                            let url = format!("{server_url_clone}/api/v1/grabber");
                            let mut req = client.post(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            match req.json(&*grab_req).send().await {
                                Ok(resp) => {
                                    let status = resp.status();
                                    if status.is_success() {
                                        if let Ok(data) =
                                            resp.json::<odm_api::GrabberResponse>().await
                                        {
                                            let _ = ui_tx.send(GuiEvent::GrabberResult(Ok(data)));
                                        } else {
                                            let _ = ui_tx.send(GuiEvent::GrabberResult(Err(
                                                "Failed to parse grabber response".into(),
                                            )));
                                        }
                                    } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                        let _ = ui_tx
                                            .send(GuiEvent::GrabberResult(Err(err.error.message)));
                                    } else {
                                        let _ = ui_tx.send(GuiEvent::GrabberResult(Err(format!(
                                            "HTTP error {status}"
                                        ))));
                                    }
                                }
                                Err(e) => {
                                    let _ = ui_tx.send(GuiEvent::GrabberResult(Err(e.to_string())));
                                }
                            }
                        }

                        UserAction::FetchPlugins => {
                            let url = format!("{server_url_clone}/api/v1/plugins");
                            let mut req = client.get(&url);
                            if let Some(ref tok) = token {
                                req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
                            }
                            match req.send().await {
                                Ok(resp) => {
                                    let status = resp.status();
                                    if status.is_success() {
                                        if let Ok(data) =
                                            resp.json::<odm_api::PluginListResponse>().await
                                        {
                                            let _ = ui_tx.send(GuiEvent::PluginsList(Ok(data)));
                                        } else {
                                            let _ = ui_tx.send(GuiEvent::PluginsList(Err(
                                                "Failed to parse plugin list response".into(),
                                            )));
                                        }
                                    } else if let Ok(err) = resp.json::<ErrorResponse>().await {
                                        let _ = ui_tx
                                            .send(GuiEvent::PluginsList(Err(err.error.message)));
                                    } else {
                                        let _ = ui_tx.send(GuiEvent::PluginsList(Err(format!(
                                            "HTTP error {status}"
                                        ))));
                                    }
                                }
                                Err(e) => {
                                    let _ = ui_tx.send(GuiEvent::PluginsList(Err(e.to_string())));
                                }
                            }
                        }

                        UserAction::Refresh => {
                            let _ = fetch_and_send_downloads(
                                &client,
                                &server_url_clone,
                                token.as_deref(),
                                &ui_tx,
                            )
                            .await;
                            let _ = fetch_and_send_categories(
                                &client,
                                &server_url_clone,
                                token.as_deref(),
                                &ui_tx,
                            )
                            .await;
                            let _ = fetch_and_send_queues(
                                &client,
                                &server_url_clone,
                                token.as_deref(),
                                &ui_tx,
                            )
                            .await;
                        }
                    }
                }
            });
        });

        Self {
            _server_url: server_url,
        }
    }
}

async fn fetch_and_send_downloads(
    client: &Client,
    server_url: &str,
    token: Option<&str>,
    ui_tx: &mpsc::UnboundedSender<GuiEvent>,
) -> Result<(), ()> {
    let url = format!("{server_url}/api/v1/downloads");
    let mut req = client.get(&url);
    if let Some(tok) = token {
        req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(list) = resp.json::<ListDownloadsResponse>().await
    {
        let _ = ui_tx.send(GuiEvent::DownloadsList(list.downloads));
        return Ok(());
    }
    Err(())
}

async fn fetch_and_send_categories(
    client: &Client,
    server_url: &str,
    token: Option<&str>,
    ui_tx: &mpsc::UnboundedSender<GuiEvent>,
) -> Result<(), ()> {
    let url = format!("{server_url}/api/v1/categories");
    let mut req = client.get(&url);
    if let Some(tok) = token {
        req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(list) = resp.json::<ListCategoriesResponse>().await
    {
        let _ = ui_tx.send(GuiEvent::CategoriesList(list.categories));
        return Ok(());
    }
    Err(())
}

async fn fetch_and_send_queues(
    client: &Client,
    server_url: &str,
    token: Option<&str>,
    ui_tx: &mpsc::UnboundedSender<GuiEvent>,
) -> Result<(), ()> {
    let url = format!("{server_url}/api/v1/queues");
    let mut req = client.get(&url);
    if let Some(tok) = token {
        req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(list) = resp.json::<ListQueuesResponse>().await
    {
        let _ = ui_tx.send(GuiEvent::QueuesList(list.queues));
        return Ok(());
    }
    Err(())
}
