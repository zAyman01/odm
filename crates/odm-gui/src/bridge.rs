//! Background Tokio bridge connecting the GUI to the odm-daemon REST & WebSocket API.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use odm_api::{
    CreateQueueRequest, DownloadDetailDto, ErrorResponse, Event, GrabberResponse,
    ListCategoriesResponse, ListDownloadsResponse, ListQueuesResponse, PluginListResponse,
    ProbeRequest, ProbeResponse, SetCategoryRequest, SettingsDto, UpdateQueueRequest,
    WsAuthRequest, WsAuthResponse,
};
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::messages::{GuiEvent, UserAction};

/// How long to wait for the daemon to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an ordinary daemon call may take.
const API_TIMEOUT: Duration = Duration::from_secs(10);
/// A site crawl runs on the daemon and answers only when it is done.
const GRAB_TIMEOUT: Duration = Duration::from_secs(300);

/// Address the daemon listens on when no override is given (matches `odm-daemon` and `odm`).
const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:6820";

/// Returns the daemon URL: the override when given, else the daemon's default address.
pub fn server_url_from(override_url: Option<String>) -> String {
    override_url.unwrap_or_else(|| DEFAULT_SERVER_URL.to_string())
}

/// Returns the daemon URL from `ODM_DAEMON_URL`, else the default address.
pub fn default_server_url() -> String {
    server_url_from(std::env::var("ODM_DAEMON_URL").ok())
}

/// Returns where the daemon writes its token (the same location `odm-daemon` and `odm` use).
pub fn default_token_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "odm").map(|dirs| dirs.config_dir().join("token"))
}

/// Reads a token file; `None` when it is missing or holds only whitespace.
pub fn read_token_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Resolves the token from `ODM_TOKEN`, else the daemon's token file.
pub fn resolve_token() -> Option<String> {
    std::env::var("ODM_TOKEN")
        .ok()
        .or_else(|| default_token_path().and_then(|path| read_token_file(&path)))
}

pub struct DaemonBridge {
    _server_url: String,
}

impl DaemonBridge {
    pub fn start(
        server_url: String,
        explicit_token: Option<String>,
        ui_tx: mpsc::UnboundedSender<GuiEvent>,
        action_rx: mpsc::UnboundedReceiver<UserAction>,
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
            runtime.block_on(run_bridge(
                server_url_clone,
                explicit_token,
                ui_tx,
                action_rx,
            ));
        });

        Self {
            _server_url: server_url,
        }
    }
}

/// Runs the bridge on the current Tokio runtime until the action channel closes.
pub async fn run_bridge(
    server_url: String,
    explicit_token: Option<String>,
    ui_tx: mpsc::UnboundedSender<GuiEvent>,
    mut action_rx: mpsc::UnboundedReceiver<UserAction>,
) {
    // No overall timeout here: each call sets its own, so a slow one is not cut short.
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| Client::new());
    let daemon = Daemon {
        client,
        server_url,
        token: explicit_token.or_else(resolve_token),
        ui_tx,
    };

    // 1. Health check & report connection status
    let mut connected = false;
    for _ in 0..10 {
        if let Ok(resp) = daemon.request(Method::GET, "/health").send().await
            && resp.status().is_success()
        {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    daemon.emit(GuiEvent::DaemonStatus(connected));

    // 2. Initial fetch of downloads, categories, queues and settings. Settings are
    //    not part of every refresh: a reload would overwrite what the user is typing.
    if connected {
        daemon.refresh_all().await;
        daemon.fetch_settings().await;
    }

    // 3. WebSocket subscriber. Events sent while it was down are gone, so each
    //    (re)connection starts from a full refresh.
    tokio::spawn(daemon.clone().follow_events());

    // 4. Every action runs in its own task, so one slow call never holds up the rest.
    while let Some(action) = action_rx.recv().await {
        let daemon = daemon.clone();
        tokio::spawn(async move { daemon.handle(action).await });
    }
}

/// Everything one daemon call needs. Cheap to clone: the client is reference counted.
#[derive(Clone)]
struct Daemon {
    client: Client,
    server_url: String,
    token: Option<String>,
    ui_tx: mpsc::UnboundedSender<GuiEvent>,
}

impl Daemon {
    /// A request to `/api/v1{path}` with the token and the default timeout.
    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!("{}/api/v1{path}", self.server_url);
        let mut request = self.client.request(method, url).timeout(API_TIMEOUT);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        request
    }

    fn emit(&self, event: GuiEvent) {
        let _ = self.ui_tx.send(event);
    }

    /// Sends `request` and decodes the answer. The error is a message for the user.
    async fn call<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, String> {
        let resp = request
            .send()
            .await
            .map_err(|e| format!("Network error: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<T>()
                .await
                .map_err(|e| format!("Unreadable daemon response: {e}"));
        }
        match resp.json::<ErrorResponse>().await {
            Ok(err) => Err(err.error.message),
            Err(_) => Err(format!("HTTP error {status}")),
        }
    }

    /// Like `call`, for answers whose body does not matter. Failures reach the UI.
    async fn call_and_report(&self, request: RequestBuilder) -> bool {
        let resp = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                self.emit(GuiEvent::ErrorMessage(format!("Network error: {e}")));
                return false;
            }
        };
        if resp.status().is_success() {
            return true;
        }
        if let Ok(err) = resp.json::<ErrorResponse>().await {
            self.emit(GuiEvent::ErrorMessage(err.error.message));
        }
        false
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        let resp = self.request(Method::GET, path).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().await.ok()
    }

    async fn fetch_downloads(&self) {
        if let Some(list) = self.get_json::<ListDownloadsResponse>("/downloads").await {
            self.emit(GuiEvent::DownloadsList(list.downloads));
        }
    }

    async fn fetch_categories(&self) {
        if let Some(list) = self.get_json::<ListCategoriesResponse>("/categories").await {
            self.emit(GuiEvent::CategoriesList(list.categories));
        }
    }

    async fn fetch_queues(&self) {
        if let Some(list) = self.get_json::<ListQueuesResponse>("/queues").await {
            self.emit(GuiEvent::QueuesList(list.queues));
        }
    }

    async fn fetch_settings(&self) {
        if let Some(settings) = self.get_json::<SettingsDto>("/settings").await {
            self.emit(GuiEvent::SettingsLoaded(Box::new(settings)));
        }
    }

    async fn fetch_detail(&self, id: i64) {
        if let Some(detail) = self
            .get_json::<DownloadDetailDto>(&format!("/downloads/{id}"))
            .await
        {
            self.emit(GuiEvent::DownloadDetail(Box::new(detail)));
        }
    }

    /// Posts a job command such as `pause`; a refusal reaches the UI.
    async fn job_command(&self, id: i64, command: &str) {
        let request = self.request(Method::POST, &format!("/downloads/{id}/{command}"));
        self.call_and_report(request).await;
    }

    async fn refresh_all(&self) {
        self.fetch_downloads().await;
        self.fetch_categories().await;
        self.fetch_queues().await;
    }

    /// Keeps the event stream open, reconnecting forever.
    async fn follow_events(self) {
        let ws_url = if let Some(stripped) = self.server_url.strip_prefix("https://") {
            format!("wss://{stripped}/api/v1/events")
        } else if let Some(stripped) = self.server_url.strip_prefix("http://") {
            format!("ws://{stripped}/api/v1/events")
        } else {
            format!("ws://{}/api/v1/events", self.server_url)
        };

        loop {
            if let Ok((mut ws_stream, _)) = connect_async(&ws_url).await {
                // First message: WebSocket authentication
                let auth_body = WsAuthRequest {
                    token: self.token.clone().unwrap_or_default(),
                };
                if let Ok(auth_json) = serde_json::to_string(&auth_body) {
                    let _ = ws_stream.send(WsMessage::Text(auth_json.into())).await;
                }

                if let Some(Ok(WsMessage::Text(resp_text))) = ws_stream.next().await
                    && let Ok(auth_resp) = serde_json::from_str::<WsAuthResponse>(&resp_text)
                    && auth_resp.authenticated
                {
                    self.refresh_all().await;
                    while let Some(msg) = ws_stream.next().await {
                        if let Ok(WsMessage::Text(text)) = msg
                            && let Ok(evt) = serde_json::from_str::<Event>(&text)
                        {
                            self.emit(GuiEvent::EngineEvent(evt));
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn handle(&self, action: UserAction) {
        match action {
            UserAction::AddDownload(req) => {
                let request = self.request(Method::POST, "/downloads").json(&req);
                if self.call_and_report(request).await {
                    self.fetch_downloads().await;
                }
            }

            UserAction::Probe(target_url) => {
                let body = ProbeRequest {
                    url: target_url,
                    user_agent: None,
                };
                let request = self.request(Method::POST, "/probe").json(&body);
                let result = self.call::<ProbeResponse>(request).await;
                self.emit(GuiEvent::ProbeResult(result));
            }

            UserAction::SelectQuality(_) => {
                // Handled in local GUI state
            }

            UserAction::Pause(id) => self.job_command(id, "pause").await,
            UserAction::Resume(id) => self.job_command(id, "resume").await,
            UserAction::Cancel(id) => self.job_command(id, "cancel").await,
            UserAction::Retry(id) => self.job_command(id, "retry").await,
            UserAction::FetchDetail(id) => self.fetch_detail(id).await,

            UserAction::Remove { id, delete_file } => {
                let path = format!("/downloads/{id}?delete_file={delete_file}");
                self.call_and_report(self.request(Method::DELETE, &path))
                    .await;
            }

            UserAction::UpdateSettings(settings) => {
                let _ = self
                    .request(Method::PATCH, "/settings")
                    .json(&settings)
                    .send()
                    .await;
            }

            UserAction::CreateQueue { name, concurrency } => {
                let body = CreateQueueRequest {
                    name,
                    max_concurrent: concurrency,
                    schedule_start: None,
                    schedule_stop: None,
                    schedule_days: None,
                };
                let request = self.request(Method::POST, "/queues").json(&body);
                if self.call_and_report(request).await {
                    self.fetch_queues().await;
                }
            }

            UserAction::SetCategory {
                name,
                directory,
                extensions,
            } => {
                let body = SetCategoryRequest {
                    directory,
                    extensions,
                };
                let request = self
                    .request(Method::PUT, &format!("/categories/{name}"))
                    .json(&body);
                if self.call_and_report(request).await {
                    self.fetch_categories().await;
                }
            }

            UserAction::UpdateQueueSchedule {
                name,
                start_time,
                stop_time,
                days,
            } => {
                // The request replaces the whole queue, so it must carry the
                // concurrency the queue has now.
                let queues = self.get_json::<ListQueuesResponse>("/queues").await;
                let Some(queue) =
                    queues.and_then(|l| l.queues.into_iter().find(|q| q.name == name))
                else {
                    self.emit(GuiEvent::ErrorMessage(format!(
                        "Could not read queue '{name}' from the daemon"
                    )));
                    return;
                };
                let body = UpdateQueueRequest {
                    max_concurrent: queue.max_concurrent,
                    schedule_start: Some(start_time),
                    schedule_stop: stop_time,
                    schedule_days: days,
                };
                let request = self
                    .request(Method::PATCH, &format!("/queues/{name}"))
                    .json(&body);
                if self.call_and_report(request).await {
                    self.fetch_queues().await;
                }
            }

            UserAction::StartGrab(grab_req) => {
                let request = self
                    .request(Method::POST, "/grabber")
                    .timeout(GRAB_TIMEOUT)
                    .json(&*grab_req);
                let result = self.call::<GrabberResponse>(request).await;
                self.emit(GuiEvent::GrabberResult(result));
            }

            UserAction::FetchPlugins => {
                let request = self.request(Method::GET, "/plugins");
                let result = self.call::<PluginListResponse>(request).await;
                self.emit(GuiEvent::PluginsList(result));
            }

            UserAction::Refresh => self.refresh_all().await,
        }
    }
}
