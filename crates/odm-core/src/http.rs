//! Single-stream and segmented HTTP backend.

use std::io::SeekFrom;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, COOKIE, ETAG,
    HeaderMap, HeaderName, HeaderValue, IF_RANGE, LAST_MODIFIED, RANGE, REFERER, USER_AGENT,
};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep};
use url::Url;

use crate::backend::{Backend, BackendError, JobContext, RunOutcome};
use crate::filename::sanitize_filename;
use crate::job::{BackendKind, JobOptions, ProbeResult, Source};
use crate::segment::Segment;

const DEFAULT_SEGMENTS: u32 = 8;
const MAX_SEGMENTS: u32 = 32;
const MIN_SEGMENT_BYTES: u64 = 1024 * 1024;
const RETRY_DELAYS_SECS: [u64; 5] = [1, 2, 4, 8, 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SegmentWork {
    index: u32,
    start: u64,
    end: u64,
    downloaded: u64,
    active: bool,
    done: bool,
}

impl SegmentWork {
    fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }

    fn remaining(&self) -> u64 {
        let pos = self.start + self.downloaded;
        if pos <= self.end {
            self.end - pos + 1
        } else {
            0
        }
    }
}

impl From<&SegmentWork> for Segment {
    fn from(w: &SegmentWork) -> Self {
        Segment {
            index: w.index,
            start: w.start,
            end: w.end,
            downloaded: w.downloaded,
        }
    }
}

fn initial_segments(size: u64, count: u32) -> Vec<SegmentWork> {
    if size == 0 {
        return Vec::new();
    }
    let count = count.clamp(1, MAX_SEGMENTS).min(size as u32).max(1);
    let mut segments = Vec::with_capacity(count as usize);
    for i in 0..count {
        let start = (i as u64) * size / (count as u64);
        let end = ((i + 1) as u64) * size / (count as u64) - 1;
        segments.push(SegmentWork {
            index: i,
            start,
            end,
            downloaded: 0,
            active: false,
            done: false,
        });
    }
    segments
}

fn claim_next(segments: &mut [SegmentWork]) -> Option<usize> {
    for (i, seg) in segments.iter_mut().enumerate() {
        if !seg.done && !seg.active {
            seg.active = true;
            return Some(i);
        }
    }
    None
}

fn split_largest(segments: &mut Vec<SegmentWork>) -> Option<usize> {
    let mut best_idx = None;
    let mut max_rem = 0;

    for (i, seg) in segments.iter().enumerate() {
        if seg.done {
            continue;
        }
        let rem = seg.remaining();
        if rem > max_rem {
            max_rem = rem;
            best_idx = Some(i);
        }
    }

    if max_rem < 2 * MIN_SEGMENT_BYTES {
        return None;
    }

    let target_idx = best_idx?;
    let pos = segments[target_idx].start + segments[target_idx].downloaded;
    let rem = segments[target_idx].end - pos + 1;
    let split_at = pos + rem / 2;
    let old_end = segments[target_idx].end;

    segments[target_idx].end = split_at - 1;
    let new_index = segments.len() as u32;
    segments.push(SegmentWork {
        index: new_index,
        start: split_at,
        end: old_end,
        downloaded: 0,
        active: false,
        done: false,
    });

    Some(new_index as usize)
}

fn segments_cover(segments: &[SegmentWork], size: u64) -> bool {
    if segments.is_empty() {
        return false;
    }
    let mut sorted: Vec<&SegmentWork> = segments.iter().collect();
    sorted.sort_by_key(|s| s.start);

    if sorted[0].start != 0 {
        return false;
    }
    for (i, seg) in sorted.iter().enumerate() {
        if seg.start > seg.end || seg.downloaded > seg.len() {
            return false;
        }
        if i > 0 && seg.start != sorted[i - 1].end + 1 {
            return false;
        }
    }
    match sorted.last() {
        Some(last) => last.end == size.saturating_sub(1),
        None => false,
    }
}

/// An HTTP backend that probes sources and downloads via single stream or segments.
#[derive(Clone)]
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

    async fn run_single_stream(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        let mut candidate_urls = vec![parse_url(ctx.job.source())?];
        for m in &ctx.job.options().mirrors {
            if let Ok(u) = Url::parse(m)
                && !candidate_urls.contains(&u)
            {
                candidate_urls.push(u);
            }
        }

        let mut response = None;
        let mut last_err = None;
        for url in candidate_urls {
            match self.request(Method::GET, url, ctx.job.options()) {
                Ok(req) => match req.send().await {
                    Ok(res) if res.status().is_success() => {
                        response = Some(res);
                        break;
                    }
                    Ok(res) => {
                        last_err = Some(BackendError::Status(res.status()));
                    }
                    Err(e) => {
                        last_err = Some(BackendError::Http(e));
                    }
                },
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        let response = match response {
            Some(res) => res,
            None => {
                return Err(last_err.unwrap_or(BackendError::Status(StatusCode::BAD_GATEWAY)));
            }
        };

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
                drop(file);
                let _ = tokio::fs::remove_file(&part_path).await;
                ctx.progress.report(0, total_bytes);
                return Ok(RunOutcome::Paused);
            }

            ctx.limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await?;
            downloaded_bytes += chunk.len() as u64;
            ctx.progress.report(downloaded_bytes, total_bytes);
        }

        file.flush().await?;
        drop(file);
        if part_path.exists() {
            tokio::fs::rename(&part_path, ctx.job.target_path()).await?;
        }
        Ok(RunOutcome::Completed)
    }

    async fn run_segmented(&self, ctx: JobContext, size: u64) -> Result<RunOutcome, BackendError> {
        tokio::fs::create_dir_all(ctx.job.target_dir()).await?;
        let part_path = ctx.job.part_path();

        let count = ctx
            .job
            .options()
            .segments
            .unwrap_or(DEFAULT_SEGMENTS)
            .clamp(1, MAX_SEGMENTS)
            .min(size as u32)
            .max(1);

        let persisted = ctx.store.list_segments(ctx.job.id())?;
        let works: Vec<SegmentWork> = persisted
            .into_iter()
            .map(|s| SegmentWork {
                index: s.index,
                start: s.start,
                end: s.end,
                downloaded: s.downloaded,
                active: false,
                done: s.is_done(),
            })
            .collect();

        let initial = if segments_cover(&works, size) {
            works
        } else {
            ctx.store.clear_segments(ctx.job.id())?;
            let initial = initial_segments(size, count);
            let initial_segs: Vec<Segment> = initial.iter().map(Segment::from).collect();
            ctx.store.replace_segments(ctx.job.id(), &initial_segs)?;
            initial
        };

        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&part_path)
            .await?;
        let meta = file.metadata().await?;
        if meta.len() != size {
            file.set_len(size).await?;
        }
        drop(file);

        let mut mirrors = Vec::new();
        if let Ok(u) = parse_url(ctx.job.source()) {
            mirrors.push(MirrorCandidate {
                url: u,
                consecutive_failures: 0,
                is_failed: false,
            });
        }
        for m in &ctx.job.options().mirrors {
            if let Ok(u) = Url::parse(m)
                && !mirrors.iter().any(|existing| existing.url == u)
            {
                mirrors.push(MirrorCandidate {
                    url: u,
                    consecutive_failures: 0,
                    is_failed: false,
                });
            }
        }

        let shared = Arc::new(Mutex::new(SharedState {
            segments: initial,
            restart_flag: false,
            restart_count: 0,
            last_saved: Instant::now(),
            mirrors,
            mirror_rr: 0,
        }));

        loop {
            let mut join_set = JoinSet::new();

            for _ in 0..count {
                let this = self.clone();
                let shared = shared.clone();
                let ctx = JobContext {
                    job: ctx.job.clone(),
                    control: ctx.control.clone(),
                    progress: ctx.progress.clone(),
                    store: ctx.store.clone(),
                    limiter: ctx.limiter.clone(),
                };
                let part_path = part_path.clone();

                join_set
                    .spawn(async move { worker_loop(this, shared, ctx, part_path, size).await });
            }

            let mut worker_error = None;
            while let Some(res) = join_set.join_next().await {
                match res {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        if worker_error.is_none() {
                            worker_error = Some(err);
                        }
                    }
                    Err(join_err) => {
                        if worker_error.is_none() {
                            worker_error = Some(BackendError::Task(join_err.to_string()));
                        }
                    }
                }
            }

            if let Some(err) = worker_error {
                return Err(err);
            }

            let (restart_flag, is_done) = {
                let s = shared.lock().unwrap_or_else(|p| p.into_inner());
                let done = s.segments.iter().all(|seg| seg.done);
                (s.restart_flag, done)
            };

            if restart_flag {
                {
                    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                    s.restart_flag = false;
                    let initial = initial_segments(size, count);
                    let initial_segs: Vec<Segment> = initial.iter().map(Segment::from).collect();
                    ctx.store.clear_segments(ctx.job.id())?;
                    ctx.store.replace_segments(ctx.job.id(), &initial_segs)?;
                    s.segments = initial;
                }
                let file = OpenOptions::new().write(true).open(&part_path).await?;
                file.set_len(0).await?;
                file.set_len(size).await?;
                drop(file);
                continue;
            }

            if ctx.control.is_cancelled() {
                save_snapshot(&ctx, &shared)?;
                return Ok(RunOutcome::Cancelled);
            }

            if ctx.control.is_paused() {
                save_snapshot(&ctx, &shared)?;
                return Ok(RunOutcome::Paused);
            }

            if is_done {
                ctx.store.clear_segments(ctx.job.id())?;
                if part_path.exists() {
                    tokio::fs::rename(&part_path, ctx.job.target_path()).await?;
                }
                return Ok(RunOutcome::Completed);
            }

            save_snapshot(&ctx, &shared)?;
            return Ok(RunOutcome::Paused);
        }
    }
}

#[derive(Debug, Clone)]
struct MirrorCandidate {
    url: Url,
    consecutive_failures: usize,
    is_failed: bool,
}

struct SharedState {
    segments: Vec<SegmentWork>,
    restart_flag: bool,
    restart_count: u32,
    last_saved: Instant,
    mirrors: Vec<MirrorCandidate>,
    mirror_rr: usize,
}

impl SharedState {
    fn next_active_url(&mut self) -> Option<Url> {
        let total = self.mirrors.len();
        if total == 0 {
            return None;
        }
        for _ in 0..total {
            let idx = self.mirror_rr % total;
            self.mirror_rr = (self.mirror_rr + 1) % total;
            if !self.mirrors[idx].is_failed {
                return Some(self.mirrors[idx].url.clone());
            }
        }
        self.mirrors
            .iter()
            .min_by_key(|m| m.consecutive_failures)
            .map(|m| m.url.clone())
    }

    fn has_other_active_mirror(&self, current: &Url) -> bool {
        self.mirrors
            .iter()
            .any(|m| &m.url != current && !m.is_failed)
    }

    fn mark_mirror_failure(&mut self, url: &Url) {
        let total = self.mirrors.len();
        for m in &mut self.mirrors {
            if &m.url == url {
                m.consecutive_failures += 1;
                if m.consecutive_failures >= 1 && total > 1 {
                    m.is_failed = true;
                }
                break;
            }
        }
    }

    fn mark_mirror_success(&mut self, url: &Url) {
        for m in &mut self.mirrors {
            if &m.url == url {
                m.consecutive_failures = 0;
                m.is_failed = false;
                break;
            }
        }
    }
}

fn save_snapshot(ctx: &JobContext, shared: &Arc<Mutex<SharedState>>) -> Result<(), BackendError> {
    let segs: Vec<Segment> = {
        let s = shared.lock().unwrap_or_else(|p| p.into_inner());
        s.segments.iter().map(Segment::from).collect()
    };
    ctx.store.replace_segments(ctx.job.id(), &segs)?;
    Ok(())
}

async fn worker_loop(
    backend: HttpBackend,
    shared: Arc<Mutex<SharedState>>,
    ctx: JobContext,
    part_path: std::path::PathBuf,
    total_size: u64,
) -> Result<(), BackendError> {
    let mut current_claim: Option<usize> = None;

    loop {
        if ctx.control.is_cancelled() || ctx.control.is_paused() {
            break;
        }
        {
            let s = shared.lock().unwrap_or_else(|p| p.into_inner());
            if s.restart_flag {
                break;
            }
        }

        let seg_idx = match current_claim.take() {
            Some(idx) => idx,
            None => {
                let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                match claim_next(&mut s.segments) {
                    Some(idx) => idx,
                    None => break,
                }
            }
        };

        let next_claim =
            download_segment(&backend, &shared, &ctx, &part_path, seg_idx, total_size).await?;
        current_claim = next_claim;
    }

    Ok(())
}

async fn download_segment(
    backend: &HttpBackend,
    shared: &Arc<Mutex<SharedState>>,
    ctx: &JobContext,
    part_path: &std::path::Path,
    seg_idx: usize,
    total_size: u64,
) -> Result<Option<usize>, BackendError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(part_path)
        .await?;

    let mut attempt = 0;
    let max_retries = ctx
        .job
        .options()
        .retry_count
        .map_or(RETRY_DELAYS_SECS.len(), |count| {
            (count as usize).min(RETRY_DELAYS_SECS.len())
        });
    loop {
        if ctx.control.is_cancelled() || ctx.control.is_paused() {
            return Ok(None);
        }

        let (curr_start, curr_end, has_progress) = {
            let s = shared.lock().unwrap_or_else(|p| p.into_inner());
            if s.restart_flag {
                return Ok(None);
            }
            if seg_idx >= s.segments.len() {
                return Ok(None);
            }
            let seg = &s.segments[seg_idx];
            let has_prog = s.segments.iter().any(|x| x.downloaded > 0);
            (seg.start + seg.downloaded, seg.end, has_prog)
        };

        if curr_start > curr_end {
            break;
        }

        let (url, has_other_mirrors) = {
            let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
            let u = match s.next_active_url() {
                Some(u) => u,
                None => parse_url(ctx.job.source())?,
            };
            let has_other = s.has_other_active_mirror(&u);
            (u, has_other)
        };

        let mut req = backend.request(Method::GET, url.clone(), ctx.job.options())?;
        req = req.header(RANGE, format!("bytes={curr_start}-{curr_end}"));
        if has_progress {
            if let Some(etag) = ctx.job.etag() {
                req = req.header(IF_RANGE, etag);
            } else if let Some(lm) = ctx.job.last_modified() {
                req = req.header(IF_RANGE, lm);
            }
        }

        let response = match req.send().await {
            Ok(res) => res,
            Err(err) => {
                {
                    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                    s.mark_mirror_failure(&url);
                }
                if has_other_mirrors {
                    continue;
                }
                if attempt < max_retries {
                    sleep(Duration::from_secs(RETRY_DELAYS_SECS[attempt])).await;
                    attempt += 1;
                    continue;
                } else {
                    return Err(BackendError::Http(err));
                }
            }
        };

        let status = response.status();
        if status.is_server_error() {
            {
                let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                s.mark_mirror_failure(&url);
            }
            if has_other_mirrors {
                continue;
            }
            if attempt < max_retries {
                sleep(Duration::from_secs(RETRY_DELAYS_SECS[attempt])).await;
                attempt += 1;
                continue;
            } else {
                return Err(BackendError::Status(status));
            }
        }

        if status == StatusCode::OK {
            let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
            if s.restart_count > 0 {
                return Err(BackendError::RangeIgnored);
            }
            s.restart_count += 1;
            s.restart_flag = true;
            return Ok(None);
        }

        if status == StatusCode::PARTIAL_CONTENT {
            {
                let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                s.mark_mirror_success(&url);
            }
            let mut response = response;
            let mut stream_err = None;

            while let Some(chunk_res) = response.chunk().await.transpose() {
                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => {
                        stream_err = Some(e);
                        break;
                    }
                };

                if ctx.control.is_cancelled() || ctx.control.is_paused() {
                    file.flush().await?;
                    return Ok(None);
                }

                ctx.limiter.acquire(chunk.len() as u64).await;

                let (write_len, should_break, pos) = {
                    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                    if s.restart_flag {
                        return Ok(None);
                    }
                    let seg = &mut s.segments[seg_idx];
                    let pos = seg.start + seg.downloaded;
                    let remaining = (seg.end + 1).saturating_sub(pos) as usize;
                    let len = chunk.len().min(remaining);
                    (len, len < chunk.len(), pos)
                };

                if write_len > 0 {
                    file.seek(SeekFrom::Start(pos)).await?;
                    file.write_all(&chunk[..write_len]).await?;

                    let (is_done, total_downloaded, should_save) = {
                        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                        let seg = &mut s.segments[seg_idx];
                        seg.downloaded += write_len as u64;
                        let done = seg.downloaded >= seg.len();
                        seg.done = done;

                        let now = Instant::now();
                        let should_save =
                            now.duration_since(s.last_saved) >= Duration::from_secs(1);
                        if should_save {
                            s.last_saved = now;
                        }
                        let total: u64 = s.segments.iter().map(|x| x.downloaded).sum();
                        (done, total, should_save)
                    };

                    ctx.progress.report(total_downloaded, Some(total_size));

                    if should_save {
                        save_snapshot(ctx, shared)?;
                    }

                    if should_break || is_done {
                        break;
                    }
                } else {
                    break;
                }
            }

            if let Some(err) = stream_err {
                if attempt < max_retries {
                    sleep(Duration::from_secs(RETRY_DELAYS_SECS[attempt])).await;
                    attempt += 1;
                    continue;
                } else {
                    return Err(BackendError::Http(err));
                }
            }

            file.flush().await?;
            break;
        } else if status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error() {
            if attempt < max_retries {
                sleep(Duration::from_secs(RETRY_DELAYS_SECS[attempt])).await;
                attempt += 1;
                continue;
            } else {
                return Err(BackendError::Status(status));
            }
        } else {
            return Err(BackendError::Status(status));
        }
    }

    let next_claim = {
        let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
        s.segments[seg_idx].active = false;
        if s.segments[seg_idx].downloaded >= s.segments[seg_idx].len() {
            s.segments[seg_idx].done = true;
        }
        if let Some(split_idx) = split_largest(&mut s.segments) {
            s.segments[split_idx].active = true;
            Some(split_idx)
        } else {
            claim_next(&mut s.segments)
        }
    };

    Ok(next_claim)
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
        if ctx.job.supports_ranges()
            && let Some(size) = ctx.job.total_bytes()
            && size > 0
        {
            self.run_segmented(ctx, size).await
        } else {
            self.run_single_stream(ctx).await
        }
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

    #[test]
    fn initial_ranges_cover_the_file() {
        for size in [10, 100, 1024 * 1024, 10 * 1024 * 1024] {
            for count in [1, 2, 4, 8, 32] {
                let segs = initial_segments(size, count);
                assert!(segments_cover(&segs, size));
            }
        }
    }

    #[test]
    fn splitting_halves_the_largest_remaining_range_without_overlap() {
        let size = 20 * 1024 * 1024;
        let mut segs = initial_segments(size, 2);
        assert_eq!(segs.len(), 2);
        let split_idx = split_largest(&mut segs);
        assert_eq!(split_idx, Some(2));
        assert_eq!(segs.len(), 3);
        assert!(segments_cover(&segs, size));
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].end, 5 * 1024 * 1024 - 1);
        assert_eq!(segs[2].start, 5 * 1024 * 1024);
        assert_eq!(segs[2].end, 10 * 1024 * 1024 - 1);
    }

    #[test]
    fn ranges_below_2_mib_are_not_split() {
        let size = 2 * 1024 * 1024 - 2;
        let mut segs = initial_segments(size, 1);
        let split_idx = split_largest(&mut segs);
        assert_eq!(split_idx, None);
        assert_eq!(segs.len(), 1);
    }
}
