//! Plugin backend implementation.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use odm_core::backend::{Backend, BackendError, JobContext, RunOutcome};
use odm_core::job::{BackendKind, JobOptions, ProbeResult, Source};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::io::AsyncWriteExt;

use crate::host::PluginHost;
use crate::protocol::ResolvedDownload;
use crate::registry::PluginRegistry;

/// Backend implementation delegating download URL resolution to external plugins.
#[derive(Clone)]
pub struct PluginBackend {
    registry: Arc<tokio::sync::RwLock<PluginRegistry>>,
    client: reqwest::Client,
}

impl PluginBackend {
    /// Creates a new `PluginBackend` with the shared plugin registry.
    pub fn new(registry: Arc<tokio::sync::RwLock<PluginRegistry>>) -> Self {
        Self {
            registry,
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new `PluginBackend` with a given HTTP client.
    pub fn with_client(
        registry: Arc<tokio::sync::RwLock<PluginRegistry>>,
        client: reqwest::Client,
    ) -> Self {
        Self { registry, client }
    }

    /// Access the shared plugin registry.
    pub fn registry(&self) -> &Arc<tokio::sync::RwLock<PluginRegistry>> {
        &self.registry
    }

    /// Asks the matching plugin to resolve `url`; the results come in the plugin's order of preference.
    async fn resolve_all(&self, url: &str) -> Result<Vec<ResolvedDownload>, BackendError> {
        let (manifest, plugin_dir) = {
            let reg = self.registry.read().await;
            let (m, d) = reg.find_for_url(url).ok_or(BackendError::Unsupported)?;
            (m.clone(), d.to_path_buf())
        };

        let response = PluginHost::resolve_default(&manifest, &plugin_dir, url)
            .await
            .map_err(|e| BackendError::Plugin(e.to_string()))?;

        if response.downloads.is_empty() {
            return Err(BackendError::Plugin(
                "plugin returned empty downloads list".to_string(),
            ));
        }
        Ok(response.downloads)
    }
}

/// Applies the job's options, then the headers the plugin resolved for this
/// download; a plugin header replaces a job header of the same name.
fn apply_headers(
    req: reqwest::RequestBuilder,
    plugin_headers: &HashMap<String, String>,
    opts: &JobOptions,
) -> Result<reqwest::RequestBuilder, BackendError> {
    let req = odm_core::apply_options(req, opts)?;

    let mut map = HeaderMap::new();
    for (k, v) in plugin_headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|_| BackendError::InvalidOptions(format!("invalid header name: {k}")))?;
        let val = HeaderValue::from_str(v)
            .map_err(|_| BackendError::InvalidOptions(format!("invalid header value for {k}")))?;
        map.insert(name, val);
    }
    Ok(req.headers(map))
}

fn parse_content_disposition_filename(cd: &str) -> Option<String> {
    for part in cd.split(';') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix("filename*=") {
            let rest = rest.trim().trim_matches('"');
            if let Some((_, encoded)) = rest.split_once("''") {
                return Some(encoded.to_string());
            }
            return Some(rest.to_string());
        }
        if let Some(rest) = trimmed.strip_prefix("filename=") {
            let filename = rest.trim().trim_matches('"').to_string();
            if !filename.is_empty() {
                return Some(filename);
            }
        }
    }
    None
}

fn extract_filename(explicit: Option<&str>, headers: &HeaderMap, url_str: &str) -> String {
    if let Some(f) = explicit {
        let trimmed = f.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(filename) = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|cd| cd.to_str().ok())
        .and_then(parse_content_disposition_filename)
    {
        return filename;
    }

    if let Some(last_segment) = reqwest::Url::parse(url_str)
        .ok()
        .and_then(|url| {
            url.path_segments()?
                .next_back()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
    {
        return last_segment;
    }

    "download".to_string()
}

fn parse_content_range_total(headers: &HeaderMap) -> Option<u64> {
    let raw = headers.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let slash = raw.rfind('/')?;
    let total_str = raw[slash + 1..].trim();
    total_str.parse::<u64>().ok()
}

#[async_trait]
impl Backend for PluginBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Plugin
    }

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError> {
        let url = source.url().ok_or(BackendError::Unsupported)?;
        let mut last_error = BackendError::Unsupported;
        for resolved in self.resolve_all(url).await? {
            match self.probe_resolved(&resolved, opts).await {
                Ok(probe) => return Ok(probe),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        let saved = ctx
            .job
            .backend_state()
            .and_then(|state| serde_json::from_str::<ResolvedDownload>(state).ok());
        let candidates = match saved {
            Some(resolved) => vec![resolved],
            None => {
                let url = ctx.job.source().url().ok_or(BackendError::Unsupported)?;
                self.resolve_all(url).await?
            }
        };

        let mut candidates = candidates.into_iter().peekable();
        let mut last_error = BackendError::Unsupported;
        while let Some(resolved) = candidates.next() {
            // A resume must come back to the result that was in progress.
            if let Ok(json) = serde_json::to_string(&resolved) {
                let _ = ctx.store.set_backend_state(ctx.job.id(), &json);
            }
            match self.download(&ctx, &resolved).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) => {
                    // The next result is a different file; it must not resume these bytes.
                    if candidates.peek().is_some() {
                        let _ = tokio::fs::remove_file(ctx.job.part_path()).await;
                    }
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }
}

impl PluginBackend {
    async fn probe_resolved(
        &self,
        resolved: &ResolvedDownload,
        opts: &JobOptions,
    ) -> Result<ProbeResult, BackendError> {
        let mut head_req = self.client.head(&resolved.url);
        head_req = apply_headers(head_req, &resolved.headers, opts)?;
        let head_res = head_req.send().await;

        let (status, headers) = match head_res {
            Ok(res) if res.status().is_success() => (res.status(), res.headers().clone()),
            _ => {
                let mut get_req = self.client.get(&resolved.url);
                get_req = apply_headers(get_req, &resolved.headers, opts)?;
                get_req = get_req.header(reqwest::header::RANGE, "bytes=0-0");
                let get_res = get_req.send().await.map_err(BackendError::Http)?;
                (get_res.status(), get_res.headers().clone())
            }
        };

        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(BackendError::Status(status));
        }

        let (total_bytes, supports_ranges) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            (parse_content_range_total(&headers), true)
        } else {
            let len = headers
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());
            let ranges = headers
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.eq_ignore_ascii_case("bytes"))
                .unwrap_or(false);
            (len, ranges)
        };

        let filename = extract_filename(resolved.filename.as_deref(), &headers, &resolved.url);
        let etag = headers
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let last_modified = headers
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        Ok(ProbeResult {
            backend: BackendKind::Plugin,
            filename,
            total_bytes,
            supports_ranges,
            etag,
            last_modified,
            content_type,
        })
    }

    async fn download(
        &self,
        ctx: &JobContext,
        resolved: &ResolvedDownload,
    ) -> Result<RunOutcome, BackendError> {
        if ctx.control.is_cancelled() {
            return Ok(RunOutcome::Cancelled);
        }
        if ctx.control.is_paused() {
            return Ok(RunOutcome::Paused);
        }

        tokio::fs::create_dir_all(ctx.job.target_dir()).await?;
        let part_path = ctx.job.part_path();
        let target_path = ctx.job.target_path();

        let mut existing_bytes = 0_u64;
        if tokio::fs::try_exists(&part_path).await.unwrap_or(false)
            && let Ok(meta) = tokio::fs::metadata(&part_path).await
        {
            existing_bytes = meta.len();
        }

        let mut req = self.client.get(&resolved.url);
        req = apply_headers(req, &resolved.headers, ctx.job.options())?;

        let can_resume = existing_bytes > 0 && ctx.job.supports_ranges();
        if can_resume {
            req = req.header(reqwest::header::RANGE, format!("bytes={existing_bytes}-"));
        }

        let response = req.send().await.map_err(BackendError::Http)?;
        let status = response.status();

        let (mut file, mut downloaded_bytes) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let f = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&part_path)
                .await?;
            (f, existing_bytes)
        } else if status.is_success() {
            let f = tokio::fs::File::create(&part_path).await?;
            (f, 0_u64)
        } else {
            return Err(BackendError::Status(status));
        };

        let total_bytes = ctx.job.total_bytes();
        let mut response = response;

        while let Some(chunk) = response.chunk().await.map_err(BackendError::Http)? {
            if ctx.control.is_cancelled() {
                file.flush().await?;
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                file.flush().await?;
                return Ok(RunOutcome::Paused);
            }

            ctx.limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await?;
            downloaded_bytes += chunk.len() as u64;
            ctx.progress.report(downloaded_bytes, total_bytes);
        }

        file.flush().await?;
        drop(file);

        if tokio::fs::try_exists(&part_path).await.unwrap_or(false) {
            tokio::fs::rename(&part_path, &target_path).await?;
        }

        Ok(RunOutcome::Completed)
    }
}
