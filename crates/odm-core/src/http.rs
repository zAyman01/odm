//! Single-stream HTTP backend.

use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, COOKIE, ETAG,
    HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED, RANGE, REFERER, USER_AGENT,
};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::backend::{Backend, BackendError, JobContext, RunOutcome};
use crate::job::{BackendKind, JobOptions, ProbeResult, Source};

/// An HTTP backend that probes sources and downloads one stream at a time.
pub struct HttpBackend {
    client: Client,
}

impl HttpBackend {
    pub fn new() -> Result<Self, BackendError> {
        Ok(Self {
            client: Client::builder().build()?,
        })
    }

    fn request(
        &self,
        method: Method,
        url: Url,
        opts: &JobOptions,
    ) -> Result<RequestBuilder, BackendError> {
        apply_options(self.client.request(method, url), opts)
    }
}

#[async_trait]
impl Backend for HttpBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Http
    }

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError> {
        let url = parse_url(source)?;
        let head = self
            .request(Method::HEAD, url.clone(), opts)?
            .send()
            .await?;

        if head.status().is_success()
            && let Some(total) = content_length(head.headers())
        {
            return Ok(probe_from(
                head.headers(),
                &url,
                Some(total),
                accepts_ranges(head.headers()),
            ));
        }

        let response = self
            .request(Method::GET, url.clone(), opts)?
            .header(RANGE, "bytes=0-0")
            .send()
            .await?;

        let status = response.status();
        let (total, supports_ranges) = if status == StatusCode::PARTIAL_CONTENT {
            (content_range_total_from(response.headers()), true)
        } else if status.is_success() {
            (
                content_length(response.headers()),
                accepts_ranges(response.headers()),
            )
        } else {
            return Err(BackendError::Status(status));
        };

        Ok(probe_from(response.headers(), &url, total, supports_ranges))
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        let url = parse_url(ctx.job.source())?;
        let response = self
            .request(Method::GET, url, ctx.job.options())?
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(BackendError::Status(status));
        }
        let total_bytes = content_length(response.headers());

        tokio::fs::create_dir_all(ctx.job.target_dir()).await?;
        let part_path = ctx.job.part_path();
        let mut file = tokio::fs::File::create(&part_path).await?;

        let mut response = response;
        let mut downloaded_bytes = 0_u64;
        while let Some(chunk) = response.chunk().await? {
            if ctx.control.is_cancelled() {
                file.flush().await?;
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                file.flush().await?;
                return Ok(RunOutcome::Paused);
            }

            file.write_all(&chunk).await?;
            downloaded_bytes += chunk.len() as u64;
            ctx.progress.report(downloaded_bytes, total_bytes);
        }

        file.flush().await?;
        drop(file);
        tokio::fs::rename(&part_path, ctx.job.target_path()).await?;
        Ok(RunOutcome::Completed)
    }
}

fn parse_url(source: &Source) -> Result<Url, BackendError> {
    let raw = source.url().ok_or(BackendError::Unsupported)?;
    Url::parse(raw).map_err(|error| BackendError::InvalidUrl(error.to_string()))
}

fn apply_options(
    mut request: RequestBuilder,
    opts: &JobOptions,
) -> Result<RequestBuilder, BackendError> {
    let mut headers = HeaderMap::new();

    for (name, value) in &opts.headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| BackendError::InvalidOptions(format!("invalid header name: {name}")))?;
        let header_value = HeaderValue::from_str(value).map_err(|_| {
            BackendError::InvalidOptions(format!("invalid header value for {name}"))
        })?;
        headers.append(header_name, header_value);
    }

    if let Some(user_agent) = &opts.user_agent {
        headers.insert(USER_AGENT, option_value("user agent", user_agent)?);
    }
    if let Some(referer) = &opts.referer {
        headers.insert(REFERER, option_value("referer", referer)?);
    }
    if let Some(cookies) = &opts.cookies {
        headers.insert(COOKIE, option_value("cookies", cookies)?);
    }

    request = request.headers(headers);
    Ok(request)
}

fn option_value(label: &str, value: &str) -> Result<HeaderValue, BackendError> {
    HeaderValue::from_str(value)
        .map_err(|_| BackendError::InvalidOptions(format!("invalid {label}")))
}

fn probe_from(
    headers: &HeaderMap,
    url: &Url,
    total_bytes: Option<u64>,
    supports_ranges: bool,
) -> ProbeResult {
    ProbeResult {
        backend: BackendKind::Http,
        filename: resolve_filename(headers, url),
        total_bytes,
        supports_ranges,
        etag: header_str(headers, ETAG).map(str::to_string),
        last_modified: header_str(headers, LAST_MODIFIED).map(str::to_string),
        content_type: header_str(headers, CONTENT_TYPE).map(str::to_string),
    }
}

fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    header_str(headers, CONTENT_LENGTH)?.parse().ok()
}

fn accepts_ranges(headers: &HeaderMap) -> bool {
    header_str(headers, ACCEPT_RANGES).is_some_and(|value| value.eq_ignore_ascii_case("bytes"))
}

fn content_range_total_from(headers: &HeaderMap) -> Option<u64> {
    header_str(headers, CONTENT_RANGE).and_then(content_range_total)
}

fn content_range_total(value: &str) -> Option<u64> {
    value.rsplit_once('/')?.1.trim().parse().ok()
}

fn resolve_filename(headers: &HeaderMap, url: &Url) -> String {
    headers
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(disposition_filename)
        .and_then(sanitize_filename)
        .or_else(|| url_filename(url))
        .unwrap_or_else(|| "download".to_string())
}

fn disposition_filename(value: &str) -> Option<&str> {
    value.split(';').skip(1).find_map(|parameter| {
        let (name, raw) = parameter.trim().split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("filename") {
            return None;
        }

        let raw = raw.trim();
        Some(
            raw.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .unwrap_or(raw),
        )
    })
}

fn url_filename(url: &Url) -> Option<String> {
    let segment = url.path_segments()?.next_back()?;
    if segment.is_empty() {
        return None;
    }

    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    sanitize_filename(&decoded)
}

fn sanitize_filename(candidate: &str) -> Option<String> {
    let leaf = candidate.rsplit(['/', '\\']).next().unwrap_or(candidate);
    let cleaned: String = leaf
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        None
    } else {
        Some(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url(value: &str) -> Url {
        match Url::parse(value) {
            Ok(url) => url,
            Err(error) => panic!("test url must parse: {error}"),
        }
    }

    fn disposition(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn reads_quoted_disposition_filename() {
        let headers = disposition("attachment; filename=\"report.pdf\"");
        assert_eq!(
            resolve_filename(&headers, &test_url("https://example.com/")),
            "report.pdf"
        );
    }

    #[test]
    fn reads_unquoted_disposition_filename() {
        let headers = disposition("attachment; filename=report.pdf");
        assert_eq!(
            resolve_filename(&headers, &test_url("https://example.com/")),
            "report.pdf"
        );
    }

    #[test]
    fn sanitizes_path_traversal() {
        assert_eq!(
            sanitize_filename("../../evil.txt").as_deref(),
            Some("evil.txt")
        );
    }

    #[test]
    fn falls_back_to_url_path_segment() {
        let url = test_url("https://example.com/files/report.pdf?token=1");
        assert_eq!(resolve_filename(&HeaderMap::new(), &url), "report.pdf");
    }

    #[test]
    fn root_url_falls_back_to_download() {
        let url = test_url("https://example.com/");
        assert_eq!(resolve_filename(&HeaderMap::new(), &url), "download");
    }

    #[test]
    fn parses_content_range_total() {
        assert_eq!(content_range_total("bytes 0-0/1234"), Some(1234));
    }
}
