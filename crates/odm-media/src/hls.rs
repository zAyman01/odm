//! HLS (HTTP Live Streaming) backend for odm.
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use aes::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
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

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

fn decrypt_segment(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, BackendError> {
    let cipher = Aes128CbcDec::new(key.into(), iv.into());
    cipher
        .decrypt_padded_vec::<Pkcs7>(data)
        .map_err(|e| BackendError::Media(format!("AES decryption failed: {e}")))
}

fn derive_iv(media_sequence: u64, segment_index: u64) -> [u8; 16] {
    let seq = media_sequence + segment_index;
    let mut iv = [0u8; 16];
    iv[8..16].copy_from_slice(&seq.to_be_bytes());
    iv
}

fn parse_iv(iv_str: &str) -> Result<[u8; 16], BackendError> {
    let clean = iv_str.trim();
    let hex_str = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
        .unwrap_or(clean);

    if hex_str.len() <= 32 && hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        let padded = format!("{:0>32}", hex_str);
        let mut iv = [0u8; 16];
        for (i, chunk) in padded.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let s = std::str::from_utf8(chunk)
                .map_err(|e| BackendError::Media(format!("invalid IV hex: {e}")))?;
            iv[i] = u8::from_str_radix(s, 16)
                .map_err(|e| BackendError::Media(format!("invalid IV hex: {e}")))?;
        }
        return Ok(iv);
    }

    if clean.len() == 16 {
        let mut iv = [0u8; 16];
        iv.copy_from_slice(clean.as_bytes());
        return Ok(iv);
    }

    Err(BackendError::Media(format!("invalid IV format: {iv_str}")))
}

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

impl HlsBackend {
    /// Downloads the initialization section named by an `EXT-X-MAP` tag.
    async fn fetch_init_segment(
        &self,
        ctx: &JobContext,
        media_url: &Url,
        map: &m3u8_rs::Map,
    ) -> Result<Vec<u8>, BackendError> {
        let url = media_url
            .join(&map.uri)
            .map_err(|e| BackendError::InvalidUrl(e.to_string()))?;
        let mut request = apply_options(self.client.get(url), ctx.job.options());
        if let Some(range) = &map.byte_range {
            let start = range.offset.unwrap_or(0);
            let end = start.saturating_add(range.length).saturating_sub(1);
            request = request.header("Range", format!("bytes={start}-{end}"));
        }
        let response = request.send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }
}

/// Identifies an init segment, so it is written once however many segments name it.
fn map_key(map: &m3u8_rs::Map) -> String {
    let range = map
        .byte_range
        .as_ref()
        .map(|range| (range.offset, range.length));
    format!("{}#{range:?}", map.uri)
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
                if !media.end_list {
                    return Err(BackendError::Media(
                        "live streams are not supported".to_string(),
                    ));
                }
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

        if !media.end_list {
            return Err(BackendError::Media(
                "live streams are not supported".to_string(),
            ));
        }

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

        let mut key_cache: HashMap<String, [u8; 16]> = HashMap::new();
        let mut active_key: Option<m3u8_rs::Key> = None;
        // The init segment in effect, and the one already written to the file.
        let mut current_map: Option<&m3u8_rs::Map> = None;
        let mut written_map: Option<String> = None;

        for (idx, seg) in media.segments.iter().enumerate() {
            let seg_idx = idx as u32;

            if let Some(ref k) = seg.key {
                active_key = Some(k.clone());
            }
            if let Some(map) = &seg.map {
                current_map = Some(map);
            }

            if resume_state.completed_segments.contains(&seg_idx) {
                written_map = current_map.map(map_key);
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

            if let Some(map) = current_map
                && written_map.as_deref() != Some(map_key(map).as_str())
            {
                let init = self.fetch_init_segment(&ctx, &media_url, map).await?;
                ctx.limiter.acquire(init.len() as u64).await;
                file.write_all(&init).await?;
                file.flush().await?;
                downloaded_bytes += init.len() as u64;
                ctx.progress.report(downloaded_bytes, ctx.job.total_bytes());
                written_map = Some(map_key(map));
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

            let data_to_write = if let Some(ref key_tag) = active_key {
                match key_tag.method {
                    m3u8_rs::KeyMethod::None => chunk.to_vec(),
                    m3u8_rs::KeyMethod::AES128 => {
                        let key_uri_str = key_tag.uri.as_deref().ok_or_else(|| {
                            BackendError::Media("AES-128 key missing URI".to_string())
                        })?;
                        let key_url = media_url
                            .join(key_uri_str)
                            .map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

                        let key = if let Some(cached) = key_cache.get(key_url.as_str()) {
                            *cached
                        } else {
                            let mut key_req = self.client.get(key_url.clone());
                            key_req = apply_options(key_req, ctx.job.options());
                            let key_res = key_req.send().await.map_err(|e| {
                                BackendError::Media(format!("failed to fetch encryption key: {e}"))
                            })?;
                            if !key_res.status().is_success() {
                                return Err(BackendError::Media(format!(
                                    "failed to fetch encryption key from {key_url}: HTTP {}",
                                    key_res.status()
                                )));
                            }
                            let key_bytes = key_res.bytes().await.map_err(|e| {
                                BackendError::Media(format!("failed to read encryption key: {e}"))
                            })?;
                            if key_bytes.len() != 16 {
                                return Err(BackendError::Media(format!(
                                    "encryption key from {key_url} must be 16 bytes, got {}",
                                    key_bytes.len()
                                )));
                            }
                            let mut k = [0u8; 16];
                            k.copy_from_slice(&key_bytes);
                            key_cache.insert(key_url.to_string(), k);
                            k
                        };

                        let iv = if let Some(ref iv_str) = key_tag.iv {
                            parse_iv(iv_str)?
                        } else {
                            derive_iv(media.media_sequence, idx as u64)
                        };

                        decrypt_segment(&chunk, &key, &iv)?
                    }
                    m3u8_rs::KeyMethod::SampleAES => {
                        return Err(BackendError::Media(
                            "SAMPLE-AES DRM streams are not supported".to_string(),
                        ));
                    }
                    ref other => {
                        return Err(BackendError::Media(format!(
                            "unsupported encryption method: {other:?}"
                        )));
                    }
                }
            } else {
                chunk.to_vec()
            };

            file.write_all(&data_to_write).await?;
            file.flush().await?;

            downloaded_bytes += data_to_write.len() as u64;
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
    odm_core::sanitize_filename(candidate).unwrap_or_else(|| "stream".to_string())
}
