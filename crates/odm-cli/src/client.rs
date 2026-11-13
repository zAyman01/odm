//! HTTP client for communicating with the odm-daemon REST API.
use std::fs;
use std::path::PathBuf;

use odm_api::{
    AddDownloadRequest, AddDownloadResponse, CategoryDto, CreateQueueRequest, DownloadDetailDto,
    ErrorResponse, GrabberRequest, GrabberResponse, HealthResponse, ListCategoriesResponse,
    ListDownloadsResponse, ListQueuesResponse, PluginListResponse, QueueDto, SetCategoryRequest,
    UpdateQueueRequest,
};
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, Response};

pub struct DaemonClient {
    client: Client,
    server_url: String,
    token: Option<String>,
}

impl DaemonClient {
    pub fn new(
        server_url: String,
        explicit_token: Option<String>,
        custom_config_dir: Option<PathBuf>,
    ) -> Self {
        let token = explicit_token
            .or_else(|| std::env::var("ODM_TOKEN").ok())
            .or_else(|| {
                let config_dir = custom_config_dir.or_else(|| {
                    directories::ProjectDirs::from("", "", "odm")
                        .map(|p| p.config_dir().to_path_buf())
                })?;
                fs::read_to_string(config_dir.join("token"))
                    .ok()
                    .map(|s| s.trim().to_string())
            });

        Self {
            client: Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, String> {
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))?;

        if status.is_success() {
            serde_json::from_slice::<T>(&bytes)
                .map_err(|e| format!("Failed to parse response: {e}"))
        } else if let Ok(err_resp) = serde_json::from_slice::<ErrorResponse>(&bytes) {
            Err(format!(
                "API Error [{}]: {}",
                err_resp.error.code, err_resp.error.message
            ))
        } else {
            Err(format!(
                "HTTP Error {status}: {}",
                String::from_utf8_lossy(&bytes)
            ))
        }
    }

    pub async fn health(&self) -> Result<HealthResponse, String> {
        let url = format!("{}/api/v1/health", self.server_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("daemon offline or unreachable at {url}: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn add_download(
        &self,
        request: &AddDownloadRequest,
    ) -> Result<AddDownloadResponse, String> {
        let url = format!("{}/api/v1/downloads", self.server_url);
        let mut req = self.client.post(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .json(request)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn list_downloads(
        &self,
        state: Option<&str>,
        queue: Option<&str>,
    ) -> Result<ListDownloadsResponse, String> {
        let mut url = format!("{}/api/v1/downloads", self.server_url);
        let mut query_parts = Vec::new();
        if let Some(st) = state {
            query_parts.push(format!("state={st}"));
        }
        if let Some(q) = queue {
            query_parts.push(format!("queue={q}"));
        }
        if !query_parts.is_empty() {
            url = format!("{url}?{}", query_parts.join("&"));
        }

        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn get_download(&self, id: i64) -> Result<DownloadDetailDto, String> {
        let url = format!("{}/api/v1/downloads/{id}", self.server_url);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn pause_download(&self, id: i64) -> Result<(), String> {
        let url = format!("{}/api/v1/downloads/{id}/pause", self.server_url);
        self.post_empty_action(&url).await
    }

    pub async fn resume_download(&self, id: i64) -> Result<(), String> {
        let url = format!("{}/api/v1/downloads/{id}/resume", self.server_url);
        self.post_empty_action(&url).await
    }

    pub async fn cancel_download(&self, id: i64) -> Result<(), String> {
        let url = format!("{}/api/v1/downloads/{id}/cancel", self.server_url);
        self.post_empty_action(&url).await
    }

    pub async fn retry_download(&self, id: i64) -> Result<(), String> {
        let url = format!("{}/api/v1/downloads/{id}/retry", self.server_url);
        self.post_empty_action(&url).await
    }

    pub async fn delete_download(&self, id: i64, delete_file: bool) -> Result<(), String> {
        let url = format!(
            "{}/api/v1/downloads/{id}?delete_file={delete_file}",
            self.server_url
        );
        let mut req = self.client.delete(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let bytes = resp.bytes().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_slice::<ErrorResponse>(&bytes) {
                Err(format!("Error: {}", err.error.message))
            } else {
                Err(format!("HTTP Error {status}"))
            }
        }
    }

    async fn post_empty_action(&self, url: &str) -> Result<(), String> {
        let mut req = self.client.post(url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let bytes = resp.bytes().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_slice::<ErrorResponse>(&bytes) {
                Err(format!("Error: {}", err.error.message))
            } else {
                Err(format!("HTTP Error {status}"))
            }
        }
    }

    pub async fn list_queues(&self) -> Result<ListQueuesResponse, String> {
        let url = format!("{}/api/v1/queues", self.server_url);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn create_queue(&self, name: &str, max_concurrent: u32) -> Result<QueueDto, String> {
        let url = format!("{}/api/v1/queues", self.server_url);
        let mut req = self.client.post(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let body = CreateQueueRequest {
            name: name.to_string(),
            max_concurrent,
            schedule_start: None,
            schedule_stop: None,
            schedule_days: None,
        };
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn set_queue_limit(
        &self,
        name: &str,
        max_concurrent: u32,
    ) -> Result<QueueDto, String> {
        let url = format!("{}/api/v1/queues/{name}", self.server_url);
        let mut req = self.client.patch(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let body = UpdateQueueRequest {
            max_concurrent,
            schedule_start: None,
            schedule_stop: None,
            schedule_days: None,
        };
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn delete_queue(&self, name: &str) -> Result<(), String> {
        let url = format!("{}/api/v1/queues/{name}", self.server_url);
        let mut req = self.client.delete(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let bytes = resp.bytes().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_slice::<ErrorResponse>(&bytes) {
                Err(format!("Error: {}", err.error.message))
            } else {
                Err(format!("HTTP Error {status}"))
            }
        }
    }

    pub async fn update_queue_schedule(
        &self,
        name: &str,
        start: &str,
        stop: Option<&str>,
        days: Option<&[String]>,
    ) -> Result<QueueDto, String> {
        let url = format!("{}/api/v1/queues/{name}", self.server_url);
        let mut req = self.client.patch(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let body = UpdateQueueRequest {
            max_concurrent: 3,
            schedule_start: Some(start.to_string()),
            schedule_stop: stop.map(|s| s.to_string()),
            schedule_days: days.map(|d| d.to_vec()),
        };
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn list_categories(&self) -> Result<ListCategoriesResponse, String> {
        let url = format!("{}/api/v1/categories", self.server_url);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn set_category(
        &self,
        name: &str,
        dir: Option<&str>,
        exts: Option<&[String]>,
    ) -> Result<CategoryDto, String> {
        let url = format!("{}/api/v1/categories/{name}", self.server_url);
        let mut req = self.client.put(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let body = SetCategoryRequest {
            directory: dir.map(|d| d.to_string()),
            extensions: exts.map(|e| e.to_vec()),
        };
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn delete_category(&self, name: &str) -> Result<(), String> {
        let url = format!("{}/api/v1/categories/{name}", self.server_url);
        let mut req = self.client.delete(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header(AUTHORIZATION, auth);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let bytes = resp.bytes().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_slice::<ErrorResponse>(&bytes) {
                Err(format!("Error: {}", err.error.message))
            } else {
                Err(format!("HTTP Error {status}"))
            }
        }
    }

    pub async fn grab(&self, req: &GrabberRequest) -> Result<GrabberResponse, String> {
        let url = format!("{}/api/v1/grabber", self.server_url);
        let mut req_builder = self.client.post(&url);
        if let Some(auth) = self.auth_header() {
            req_builder = req_builder.header(AUTHORIZATION, auth);
        }
        let resp = req_builder
            .json(req)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }

    pub async fn list_plugins(&self) -> Result<PluginListResponse, String> {
        let url = format!("{}/api/v1/plugins", self.server_url);
        let mut req_builder = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req_builder = req_builder.header(AUTHORIZATION, auth);
        }
        let resp = req_builder
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        self.handle_response(resp).await
    }
}
