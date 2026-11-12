//! HLS (HTTP Live Streaming) backend for odm.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use odm_core::backend::{Backend, BackendError, JobContext, RunOutcome};
use odm_core::job::{BackendKind, JobOptions, ProbeResult, Source};
use reqwest::header::{COOKIE, HeaderName, HeaderValue, REFERER, USER_AGENT};
use reqwest::{Client, RequestBuilder};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::mux;
use crate::quality::QualityInfo;
use crate::state::HlsResumeState;

/// Backend implementation for HLS streams (`.m3u8`).
pub struct HlsBackend {
    client: Client,
    ffmpeg_path: Option<PathBuf>,
}

impl HlsBackend {
    /// Creates a new HLS backend with an HTTP client and optional ffmpeg path.
    pub fn new(http_client: Client, ffmpeg_path: Option<PathBuf>) -> Self {
        Self {
            client: http_client,
            ffmpeg_path,
        }
    }

    /// Queries rendition qualities from a master playlist.
    pub async fn probe_qualities(&self, source: &Source) -> Result<Vec<QualityInfo>, BackendError> {
        let url_str = source.url().ok_or(BackendError::Unsupported)?;
        let url = Url::parse(url_str).map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

        let res = self.client.get(url).send().await?.error_for_status()?;
        let bytes = res.bytes().await?;
        let text = String::from_utf8_lossy(&bytes);

        if is_drm_protected(&text) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }

        let playlist = m3u8_rs::parse_playlist_res(&bytes)
            .map_err(|e| BackendError::Media(format!("failed to parse playlist: {e}")))?;

        match playlist {
            m3u8_rs::Playlist::MasterPlaylist(master) => {
                let mut qualities = Vec::new();
                for variant in master.variants {
                    let resolution_str = variant
                        .resolution
                        .as_ref()
                        .map(|r| format!("{}x{}", r.width, r.height));
                    let id = if let Some(res) = &variant.resolution {
                        format!("{}p", res.height)
                    } else {
                        format!("{}k", variant.bandwidth / 1000)
                    };
                    let label = if let Some(res) = &variant.resolution {
                        format!("{}p - {} kbps", res.height, variant.bandwidth / 1000)
                    } else {
                        format!("{} kbps", variant.bandwidth / 1000)
                    };
                    qualities.push(QualityInfo {
                        id,
                        resolution: resolution_str,
                        bandwidth: Some(variant.bandwidth),
                        codecs: variant.codecs,
                        label,
                    });
                }
                Ok(qualities)
            }
            m3u8_rs::Playlist::MediaPlaylist(_) => Ok(Vec::new()),
        }
    }
}

#[async_trait]
impl Backend for HlsBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Hls
    }

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError> {
        let url_str = source.url().ok_or(BackendError::Unsupported)?;
        let url = Url::parse(url_str).map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

        let mut req = self.client.get(url.clone());
        req = apply_options(req, opts);
        let res = req.send().await?.error_for_status()?;
        let bytes = res.bytes().await?;
        let text = String::from_utf8_lossy(&bytes);

        if is_drm_protected(&text) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }

        let playlist = m3u8_rs::parse_playlist_res(&bytes)
            .map_err(|e| BackendError::Media(format!("failed to parse playlist: {e}")))?;

        let default_ext = if self.ffmpeg_path.as_ref().is_some_and(|p| p.exists()) {
            "mp4"
        } else {
            "ts"
        };

        match playlist {
            m3u8_rs::Playlist::MasterPlaylist(_) => {
                let filename = derive_filename(&url, default_ext);
                Ok(ProbeResult {
                    backend: BackendKind::Hls,
                    filename,
                    total_bytes: None,
                    supports_ranges: false,
                    etag: None,
                    last_modified: None,
                    content_type: Some("application/vnd.apple.mpegurl".to_string()),
                })
            }
            m3u8_rs::Playlist::MediaPlaylist(media) => {
                let filename = derive_filename(&url, default_ext);
                let total_bytes = estimate_media_size(&self.client, &url, &media.segments).await;
                Ok(ProbeResult {
                    backend: BackendKind::Hls,
                    filename,
                    total_bytes,
                    supports_ranges: false,
                    etag: None,
                    last_modified: None,
                    content_type: Some("application/vnd.apple.mpegurl".to_string()),
                })
            }
        }
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        if ctx.control.is_cancelled() {
            return Ok(RunOutcome::Cancelled);
        }
        if ctx.control.is_paused() {
            return Ok(RunOutcome::Paused);
        }

        let url_str = ctx.job.source().url().ok_or(BackendError::Unsupported)?;
        let base_url = Url::parse(url_str).map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

        let mut req = self.client.get(base_url.clone());
        req = apply_options(req, ctx.job.options());
        let res = req.send().await?.error_for_status()?;
        let bytes = res.bytes().await?;
        let text = String::from_utf8_lossy(&bytes);

        if is_drm_protected(&text) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }

        let playlist = m3u8_rs::parse_playlist_res(&bytes)
            .map_err(|e| BackendError::Media(format!("failed to parse playlist: {e}")))?;

        let (media, media_url, selected_quality) = match playlist {
            m3u8_rs::Playlist::MasterPlaylist(master) => {
                let variant =
                    select_variant(&master.variants, ctx.job.options().quality.as_deref())
                        .ok_or_else(|| {
                            BackendError::Media(
                                "no variant streams found in master playlist".to_string(),
                            )
                        })?;

                let quality_id = if let Some(res) = &variant.resolution {
                    format!("{}p", res.height)
                } else {
                    format!("{}k", variant.bandwidth / 1000)
                };

                let variant_url = base_url
                    .join(&variant.uri)
                    .map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

                let mut v_req = self.client.get(variant_url.clone());
                v_req = apply_options(v_req, ctx.job.options());
                let v_res = v_req.send().await?.error_for_status()?;
                let v_bytes = v_res.bytes().await?;
                let v_text = String::from_utf8_lossy(&v_bytes);

                if is_drm_protected(&v_text) {
                    return Err(BackendError::Media(
                        "DRM-protected streams are not supported".to_string(),
                    ));
                }

                let sub_playlist = m3u8_rs::parse_playlist_res(&v_bytes).map_err(|e| {
                    BackendError::Media(format!("failed to parse variant playlist: {e}"))
                })?;

                match sub_playlist {
                    m3u8_rs::Playlist::MediaPlaylist(m) => (m, variant_url, Some(quality_id)),
                    m3u8_rs::Playlist::MasterPlaylist(_) => {
                        return Err(BackendError::Media(
                            "nested master playlists not supported".to_string(),
                        ));
                    }
                }
            }
            m3u8_rs::Playlist::MediaPlaylist(media) => (media, base_url, None),
        };

        let total_segments = media.segments.len() as u32;

        let saved_state_str = ctx
            .store
            .get_backend_state(ctx.job.id())
            .ok()
            .flatten()
            .or_else(|| ctx.job.backend_state().map(|s| s.to_string()));

        let mut resume_state = saved_state_str
            .as_deref()
            .and_then(|s| serde_json::from_str::<HlsResumeState>(s).ok())
            .unwrap_or_else(|| HlsResumeState {
                completed_segments: Vec::new(),
                selected_quality: selected_quality.clone(),
                total_segments,
            });
        resume_state.total_segments = total_segments;
        if resume_state.selected_quality.is_none() {
            resume_state.selected_quality = selected_quality;
        }

        tokio::fs::create_dir_all(ctx.job.target_dir()).await?;
        let part_path = ctx.job.part_path();

        let mut file = if resume_state.completed_segments.is_empty() {
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&part_path)
                .await?
        } else {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&part_path)
                .await?
        };

        let mut downloaded_bytes = file.metadata().await?.len();
        ctx.progress.report(downloaded_bytes, ctx.job.total_bytes());

        for (idx, seg) in media.segments.iter().enumerate() {
            let seg_idx = idx as u32;
            if resume_state.completed_segments.contains(&seg_idx) {
                continue;
            }

            if ctx.control.is_cancelled() {
                file.flush().await?;
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                file.flush().await?;
                let state_json = serde_json::to_string(&resume_state)
                    .map_err(|e| BackendError::Media(e.to_string()))?;
                ctx.store.set_backend_state(ctx.job.id(), &state_json)?;
                return Ok(RunOutcome::Paused);
            }

            let seg_url = media_url
                .join(&seg.uri)
                .map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

            let mut seg_req = self.client.get(seg_url);
            seg_req = apply_options(seg_req, ctx.job.options());
            if let Some(ref range) = seg.byte_range {
                let start = range.offset.unwrap_or(0);
                let end = start.saturating_add(range.length).saturating_sub(1);
                seg_req = seg_req.header("Range", format!("bytes={start}-{end}"));
            }

            let seg_res = seg_req.send().await?.error_for_status()?;
            let chunk = seg_res.bytes().await?;

            ctx.limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await?;
            file.flush().await?;

            downloaded_bytes += chunk.len() as u64;
            ctx.progress.report(downloaded_bytes, ctx.job.total_bytes());

            resume_state.completed_segments.push(seg_idx);
            let state_json = serde_json::to_string(&resume_state)
                .map_err(|e| BackendError::Media(e.to_string()))?;
            ctx.store.set_backend_state(ctx.job.id(), &state_json)?;

            if ctx.control.is_cancelled() {
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                return Ok(RunOutcome::Paused);
            }
        }

        file.flush().await?;
        drop(file);

        let target_path = ctx.job.target_path();
        let is_mp4 = target_path.extension().and_then(|e| e.to_str()) == Some("mp4");

        if let Some(ffmpeg) = self.ffmpeg_path.as_deref().filter(|p| p.exists() && is_mp4) {
            mux::mux_to_mp4(&part_path, &target_path, ffmpeg)
                .await
                .map_err(|e| BackendError::Media(format!("ffmpeg mux failed: {e}")))?;
            let _ = tokio::fs::remove_file(&part_path).await;
            return Ok(RunOutcome::Completed);
        }

        tokio::fs::rename(&part_path, &target_path).await?;
        Ok(RunOutcome::Completed)
    }
}

async fn estimate_media_size(
    client: &Client,
    url: &Url,
    segments: &[m3u8_rs::MediaSegment],
) -> Option<u64> {
    let first = segments.first()?;
    let first_url = url.join(&first.uri).ok()?;
    let res = client.head(first_url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let cl: u64 = res
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    Some(cl.saturating_mul(segments.len() as u64))
}

fn apply_options(mut builder: RequestBuilder, opts: &JobOptions) -> RequestBuilder {
    if let Some(ref ua) = opts.user_agent {
        builder = builder.header(USER_AGENT, ua);
    }
    if let Some(ref ref_url) = opts.referer {
        builder = builder.header(REFERER, ref_url);
    }
    if let Some(ref cookies) = opts.cookies {
        builder = builder.header(COOKIE, cookies);
    }
    for (name, value) in &opts.headers {
        if let (Ok(hname), Ok(hval)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            builder = builder.header(hname, hval);
        }
    }
    builder
}

fn is_drm_protected(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if text.contains("METHOD=SAMPLE-AES") || text.contains("METHOD=\"SAMPLE-AES\"") {
        return true;
    }
    if lower.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed") || lower.contains("com.widevine") {
        return true;
    }
    if lower.contains("9a04f079-9840-4286-ab92-e65be0885f95") {
        return true;
    }
    if lower.contains("skd://") || lower.contains("com.apple.streamingkeydelivery") {
        return true;
    }
    false
}

fn select_variant<'a>(
    variants: &'a [m3u8_rs::VariantStream],
    quality_pref: Option<&str>,
) -> Option<&'a m3u8_rs::VariantStream> {
    if variants.is_empty() {
        return None;
    }
    if let Some(pref) = quality_pref {
        let pref_trimmed = pref.trim();
        for v in variants {
            if let Some(res) = &v.resolution {
                let res_str = format!("{}x{}", res.width, res.height);
                let p_str = format!("{}p", res.height);
                if pref_trimmed.eq_ignore_ascii_case(&res_str)
                    || pref_trimmed.eq_ignore_ascii_case(&p_str)
                {
                    return Some(v);
                }
            }
        }
        for v in variants {
            let bw_str = v.bandwidth.to_string();
            let kbps = format!("{}k", v.bandwidth / 1000);
            if pref_trimmed == bw_str || pref_trimmed.eq_ignore_ascii_case(&kbps) {
                return Some(v);
            }
        }
        for v in variants {
            if v.uri.contains(pref_trimmed) {
                return Some(v);
            }
        }
    }
    variants.iter().max_by_key(|v| v.bandwidth)
}

fn derive_filename(url: &Url, default_ext: &str) -> String {
    let raw = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or("stream");

    let stem = if let Some(stripped) = raw.strip_suffix(".m3u8") {
        if stripped.is_empty() {
            "stream"
        } else {
            stripped
        }
    } else {
        raw
    };

    let cleaned = sanitize_filename(stem);
    if cleaned.ends_with(".ts") || cleaned.ends_with(".mp4") {
        cleaned
    } else {
        format!("{cleaned}.{default_ext}")
    }
}

fn sanitize_filename(candidate: &str) -> String {
    let leaf = candidate.rsplit(['/', '\\']).next().unwrap_or(candidate);
    let cleaned: String = leaf.chars().filter(|c| !c.is_control()).collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "stream".to_string()
    } else {
        cleaned
    }
}
