//! DASH (Dynamic Adaptive Streaming over HTTP) backend for odm.
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
use crate::state::DashResumeState;

#[derive(Debug, Clone)]
struct DashSegment {
    url: Url,
    byte_range: Option<(u64, u64)>,
}

macro_rules! is_video_adaptation {
    ($ad:expr) => {
        $ad.contentType.as_deref() == Some("video")
            || $ad
                .mimeType
                .as_deref()
                .is_some_and(|m| m.starts_with("video/"))
            || $ad.width.is_some()
            || $ad.height.is_some()
            || $ad.representations.iter().any(|r| {
                r.contentType.as_deref() == Some("video")
                    || r.mimeType
                        .as_deref()
                        .is_some_and(|m| m.starts_with("video/"))
                    || r.width.is_some()
                    || r.height.is_some()
            })
    };
}

macro_rules! is_audio_adaptation {
    ($ad:expr) => {
        $ad.contentType.as_deref() == Some("audio")
            || $ad
                .mimeType
                .as_deref()
                .is_some_and(|m| m.starts_with("audio/"))
            || $ad.audioSamplingRate.is_some()
            || $ad.representations.iter().any(|r| {
                r.contentType.as_deref() == Some("audio")
                    || r.mimeType
                        .as_deref()
                        .is_some_and(|m| m.starts_with("audio/"))
                    || r.audioSamplingRate.is_some()
            })
    };
}

macro_rules! check_drm_in_mpd {
    ($mpd:expr) => {{
        let mut drm_found = false;
        for cp in &$mpd.ContentProtection {
            if is_drm_uri(&cp.schemeIdUri) {
                drm_found = true;
                break;
            }
        }
        if !drm_found {
            for p in &$mpd.periods {
                for cp in &p.ContentProtection {
                    if is_drm_uri(&cp.schemeIdUri) {
                        drm_found = true;
                        break;
                    }
                }
                if drm_found {
                    break;
                }
                for ad in &p.adaptations {
                    for cp in &ad.ContentProtection {
                        if is_drm_uri(&cp.schemeIdUri) {
                            drm_found = true;
                            break;
                        }
                    }
                    if drm_found {
                        break;
                    }
                    for rep in &ad.representations {
                        for cp in &rep.ContentProtection {
                            if is_drm_uri(&cp.schemeIdUri) {
                                drm_found = true;
                                break;
                            }
                        }
                        if drm_found {
                            break;
                        }
                    }
                    if drm_found {
                        break;
                    }
                }
                if drm_found {
                    break;
                }
            }
        }
        drm_found
    }};
}

macro_rules! extract_segments_from_rep {
    ($manifest_url:expr, $mpd:expr, $period:expr, $adaptation:expr, $rep:expr) => {{
        let mut segments: Vec<DashSegment> = Vec::new();
        let mut base = $manifest_url.clone();
        if let Some(b) = $mpd.base_url.first() {
            if let Ok(u) = base.join(&b.base) {
                base = u;
            }
        }
        if let Some(b) = $period.BaseURL.first() {
            if let Ok(u) = base.join(&b.base) {
                base = u;
            }
        }
        if let Some(b) = $adaptation.BaseURL.first() {
            if let Ok(u) = base.join(&b.base) {
                base = u;
            }
        }
        if let Some(b) = $rep.BaseURL.first() {
            if let Ok(u) = base.join(&b.base) {
                base = u;
            }
        }

        let rep_id = $rep.id.as_deref().unwrap_or("0");
        let seg_list = $rep
            .SegmentList
            .as_ref()
            .or($adaptation.SegmentList.as_ref())
            .or($period.SegmentList.as_ref());
        let seg_template = $rep
            .SegmentTemplate
            .as_ref()
            .or($adaptation.SegmentTemplate.as_ref())
            .or($period.SegmentTemplate.as_ref());

        if let Some(sl) = seg_list {
            if let Some(init) = &sl.Initialization {
                if let Some(src) = &init.sourceURL {
                    if let Ok(u) = base.join(src) {
                        segments.push(DashSegment {
                            url: u,
                            byte_range: init.range.as_deref().and_then(parse_byte_range),
                        });
                    }
                }
            }
            for seg_url in &sl.segment_urls {
                if let Some(m) = &seg_url.media {
                    if let Ok(u) = base.join(m) {
                        segments.push(DashSegment {
                            url: u,
                            byte_range: seg_url.mediaRange.as_deref().and_then(parse_byte_range),
                        });
                    }
                }
            }
        } else if let Some(st) = seg_template {
            if let Some(init_tmpl) = &st.initialization {
                let resolved = resolve_template(init_tmpl, rep_id, None, None, $rep.bandwidth);
                if let Ok(u) = base.join(&resolved) {
                    segments.push(DashSegment {
                        url: u,
                        byte_range: None,
                    });
                }
            } else if let Some(init) = &st.Initialization {
                if let Some(src) = &init.sourceURL {
                    let resolved = resolve_template(src, rep_id, None, None, $rep.bandwidth);
                    if let Ok(u) = base.join(&resolved) {
                        segments.push(DashSegment {
                            url: u,
                            byte_range: init.range.as_deref().and_then(parse_byte_range),
                        });
                    }
                }
            }

            if let Some(media_tmpl) = &st.media {
                if let Some(timeline) = &st.SegmentTimeline {
                    let mut current_number = st.startNumber.unwrap_or(1);
                    let mut current_time = 0u64;
                    for s in &timeline.segments {
                        if let Some(t) = s.t {
                            current_time = t;
                        }
                        let repeats = s.r.unwrap_or(0).max(0);
                        for _ in 0..=repeats {
                            let resolved = resolve_template(
                                media_tmpl,
                                rep_id,
                                Some(current_number),
                                Some(current_time),
                                $rep.bandwidth,
                            );
                            if let Ok(u) = base.join(&resolved) {
                                segments.push(DashSegment {
                                    url: u,
                                    byte_range: None,
                                });
                            }
                            current_number += 1;
                            current_time += s.d;
                        }
                    }
                } else {
                    let start_num = st.startNumber.unwrap_or(1);
                    let count = if let Some(end_num) = st.endNumber {
                        end_num.saturating_sub(start_num) + 1
                    } else if let (Some(duration), Some(period_dur)) = (
                        st.duration,
                        // The nearest ancestor that states a duration wins.
                        $period.duration.or($mpd.mediaPresentationDuration),
                    ) {
                        let timescale = st.timescale.unwrap_or(1) as f64;
                        let seg_dur_secs = duration / timescale;
                        if seg_dur_secs > 0.0 {
                            (period_dur.as_secs_f64() / seg_dur_secs).ceil() as u64
                        } else {
                            1
                        }
                    } else {
                        1
                    };

                    for i in 0..count {
                        let num = start_num + i;
                        let resolved =
                            resolve_template(media_tmpl, rep_id, Some(num), None, $rep.bandwidth);
                        if let Ok(u) = base.join(&resolved) {
                            segments.push(DashSegment {
                                url: u,
                                byte_range: None,
                            });
                        }
                    }
                }
            }
        } else {
            if let Some(sb) = &$rep.SegmentBase {
                if let Some(init) = &sb.Initialization {
                    if let Some(src) = &init.sourceURL {
                        if let Ok(u) = base.join(src) {
                            segments.push(DashSegment {
                                url: u,
                                byte_range: init.range.as_deref().and_then(parse_byte_range),
                            });
                        }
                    }
                }
            }
            if !$rep.BaseURL.is_empty() {
                for b in &$rep.BaseURL {
                    if let Ok(u) = base.join(&b.base) {
                        segments.push(DashSegment {
                            url: u,
                            byte_range: b.byte_range.as_deref().and_then(parse_byte_range),
                        });
                    }
                }
            } else if base != *$manifest_url {
                segments.push(DashSegment {
                    url: base,
                    byte_range: None,
                });
            }
        }

        segments
    }};
}

/// Backend implementation for DASH streams (`.mpd`).
pub struct DashBackend {
    client: Client,
    ffmpeg_path: Option<PathBuf>,
}

impl DashBackend {
    /// Creates a new DASH backend with an HTTP client and optional ffmpeg path.
    pub fn new(http_client: Client, ffmpeg_path: Option<PathBuf>) -> Self {
        Self {
            client: http_client,
            ffmpeg_path,
        }
    }

    /// Queries rendition qualities from the MPD manifest.
    pub async fn probe_qualities(&self, source: &Source) -> Result<Vec<QualityInfo>, BackendError> {
        let url_str = source.url().ok_or(BackendError::Unsupported)?;
        let url = Url::parse(url_str).map_err(|e| BackendError::InvalidUrl(e.to_string()))?;

        let res = self
            .client
            .get(url.clone())
            .send()
            .await?
            .error_for_status()?;
        let bytes = res.bytes().await?;
        let text = String::from_utf8_lossy(&bytes);

        if is_drm_protected(&text) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }

        let downloader = dash_mpd::DashDownloader::new(url.as_str());
        let mpd = dash_mpd::parse_resolving_xlinks(&downloader, &bytes)
            .await
            .map_err(|e| BackendError::Media(format!("failed to parse manifest: {e}")))?;

        if check_drm_in_mpd!(mpd) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }

        let mut qualities = Vec::new();

        for period in &mpd.periods {
            for adaptation in &period.adaptations {
                if is_video_adaptation!(adaptation) {
                    for rep in &adaptation.representations {
                        let w = rep.width.or(adaptation.width);
                        let h = rep.height.or(adaptation.height);
                        let resolution = match (w, h) {
                            (Some(width), Some(height)) => Some(format!("{width}x{height}")),
                            _ => None,
                        };
                        let bandwidth = rep.bandwidth.or(adaptation.minBandwidth);
                        let codecs = rep.codecs.clone().or_else(|| adaptation.codecs.clone());

                        let id = if let Some(height) = h {
                            format!("{height}p")
                        } else if let Some(ref rid) = rep.id {
                            rid.clone()
                        } else if let Some(bw) = bandwidth {
                            format!("{}k", bw / 1000)
                        } else {
                            "video".to_string()
                        };

                        let label = if let Some(height) = h {
                            if let Some(bw) = bandwidth {
                                format!("{height}p - {} kbps", bw / 1000)
                            } else {
                                format!("{height}p")
                            }
                        } else if let Some(bw) = bandwidth {
                            format!("{} kbps", bw / 1000)
                        } else {
                            id.clone()
                        };

                        qualities.push(QualityInfo {
                            id,
                            resolution,
                            bandwidth,
                            codecs,
                            label,
                        });
                    }
                }
            }
        }

        Ok(qualities)
    }
}

impl DashBackend {
    /// Streams one segment to `file` as it arrives, so a large single-file
    /// representation never sits in memory. A failed segment is cut off the
    /// file again, so a later resume does not append after a partial one.
    async fn download_segment(
        &self,
        ctx: &JobContext,
        seg: &DashSegment,
        file: &mut tokio::fs::File,
        downloaded_bytes: &mut u64,
    ) -> Result<(), BackendError> {
        let start_len = file.metadata().await?.len();
        let result = self.stream_segment(ctx, seg, file, downloaded_bytes).await;
        if result.is_err() {
            let _ = file.set_len(start_len).await;
        }
        result
    }

    async fn stream_segment(
        &self,
        ctx: &JobContext,
        seg: &DashSegment,
        file: &mut tokio::fs::File,
        downloaded_bytes: &mut u64,
    ) -> Result<(), BackendError> {
        let mut request = self.client.get(seg.url.clone());
        request = apply_options(request, ctx.job.options());
        if let Some((start, end)) = seg.byte_range {
            request = request.header("Range", format!("bytes={start}-{end}"));
        }
        let mut response = request.send().await?.error_for_status()?;
        while let Some(chunk) = response.chunk().await? {
            ctx.limiter.acquire(chunk.len() as u64).await;
            file.write_all(&chunk).await?;
            file.flush().await?;
            *downloaded_bytes += chunk.len() as u64;
            ctx.progress
                .report(*downloaded_bytes, ctx.job.total_bytes());
        }
        Ok(())
    }
}

#[async_trait]
impl Backend for DashBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Dash
    }

    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError> {
        let _ffmpeg = match &self.ffmpeg_path {
            Some(p) if p.exists() => p.clone(),
            _ => {
                return Err(BackendError::Media(
                    "ffmpeg is required for DASH downloads".to_string(),
                ));
            }
        };

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

        let downloader = dash_mpd::DashDownloader::new(url.as_str());
        let mpd = dash_mpd::parse_resolving_xlinks(&downloader, &bytes)
            .await
            .map_err(|e| BackendError::Media(format!("failed to parse manifest: {e}")))?;

        if check_drm_in_mpd!(mpd) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }
        reject_live(mpd.mpdtype.as_deref())?;

        let filename = derive_filename(&url, "mp4");

        Ok(ProbeResult {
            backend: BackendKind::Dash,
            filename,
            total_bytes: None,
            supports_ranges: false,
            etag: None,
            last_modified: None,
            content_type: Some("application/dash+xml".to_string()),
        })
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        if ctx.control.is_cancelled() {
            return Ok(RunOutcome::Cancelled);
        }
        if ctx.control.is_paused() {
            return Ok(RunOutcome::Paused);
        }

        let ffmpeg = match &self.ffmpeg_path {
            Some(p) if p.exists() => p.clone(),
            _ => {
                return Err(BackendError::Media(
                    "ffmpeg is required for DASH downloads".to_string(),
                ));
            }
        };

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

        let downloader = dash_mpd::DashDownloader::new(base_url.as_str());
        let mpd = dash_mpd::parse_resolving_xlinks(&downloader, &bytes)
            .await
            .map_err(|e| BackendError::Media(format!("failed to parse manifest: {e}")))?;

        if check_drm_in_mpd!(mpd) {
            return Err(BackendError::Media(
                "DRM-protected streams are not supported".to_string(),
            ));
        }
        reject_live(mpd.mpdtype.as_deref())?;

        let quality_pref = ctx.job.options().quality.as_deref();

        // 1. Select video representation and collect its segments
        let mut video_segments = Vec::new();
        let mut selected_quality = None;

        let mut best_video: Option<(u64, Vec<DashSegment>, String)> = None;
        let mut preferred_video: Option<(Vec<DashSegment>, String)> = None;

        for period in &mpd.periods {
            for adaptation in &period.adaptations {
                if is_video_adaptation!(adaptation) {
                    for rep in &adaptation.representations {
                        let rep_id = rep.id.as_deref().unwrap_or("video");
                        let h = rep.height.or(adaptation.height);
                        let w = rep.width.or(adaptation.width);
                        let bw = rep.bandwidth.unwrap_or(0);

                        let q_id = if let Some(height) = h {
                            format!("{height}p")
                        } else {
                            rep_id.to_string()
                        };

                        let is_match = if let Some(pref) = quality_pref {
                            let p = pref.trim();
                            p.eq_ignore_ascii_case(&q_id)
                                || p.eq_ignore_ascii_case(rep_id)
                                || (h.is_some()
                                    && p.eq_ignore_ascii_case(&format!("{}p", h.unwrap_or(0))))
                                || (w.is_some()
                                    && h.is_some()
                                    && p == format!("{}x{}", w.unwrap_or(0), h.unwrap_or(0)))
                                || p == bw.to_string()
                                || p.eq_ignore_ascii_case(&format!("{}k", bw / 1000))
                        } else {
                            false
                        };

                        let segs =
                            extract_segments_from_rep!(&base_url, &mpd, period, adaptation, rep);

                        if is_match && preferred_video.is_none() {
                            preferred_video = Some((segs.clone(), q_id.clone()));
                        }

                        if best_video
                            .as_ref()
                            .is_none_or(|(best_bw, _, _)| bw > *best_bw)
                        {
                            best_video = Some((bw, segs, q_id));
                        }
                    }
                }
            }
        }

        if let Some((segs, q_id)) =
            preferred_video.or_else(|| best_video.map(|(_, segs, q_id)| (segs, q_id)))
        {
            video_segments = segs;
            selected_quality = Some(q_id);
        }

        // 2. Select audio representation and collect its segments
        let mut audio_segments = Vec::new();
        let mut best_audio: Option<(u64, Vec<DashSegment>)> = None;

        for period in &mpd.periods {
            for adaptation in &period.adaptations {
                if is_audio_adaptation!(adaptation) {
                    for rep in &adaptation.representations {
                        let bw = rep.bandwidth.unwrap_or(0);
                        let segs =
                            extract_segments_from_rep!(&base_url, &mpd, period, adaptation, rep);
                        if best_audio
                            .as_ref()
                            .is_none_or(|(best_bw, _)| bw >= *best_bw)
                        {
                            best_audio = Some((bw, segs));
                        }
                    }
                }
            }
        }

        if let Some((_, segs)) = best_audio {
            audio_segments = segs;
        }

        let total_video_segments = video_segments.len() as u32;
        let total_audio_segments = audio_segments.len() as u32;

        let saved_state_str = ctx
            .store
            .get_backend_state(ctx.job.id())
            .ok()
            .flatten()
            .or_else(|| ctx.job.backend_state().map(|s| s.to_string()));

        let mut resume_state = saved_state_str
            .as_deref()
            .and_then(|s| serde_json::from_str::<DashResumeState>(s).ok())
            .unwrap_or_else(|| DashResumeState {
                completed_video_segments: Vec::new(),
                completed_audio_segments: Vec::new(),
                selected_quality: selected_quality.clone(),
                total_video_segments,
                total_audio_segments,
            });
        resume_state.total_video_segments = total_video_segments;
        resume_state.total_audio_segments = total_audio_segments;
        if resume_state.selected_quality.is_none() {
            resume_state.selected_quality = selected_quality;
        }

        tokio::fs::create_dir_all(ctx.job.target_dir()).await?;
        let part_path = ctx.job.part_path();

        let mut v_tmp_os = part_path.clone().into_os_string();
        v_tmp_os.push(".v.tmp");
        let video_tmp_path = PathBuf::from(v_tmp_os);

        let mut a_tmp_os = part_path.clone().into_os_string();
        a_tmp_os.push(".a.tmp");
        let audio_tmp_path = PathBuf::from(a_tmp_os);

        let mut video_file = if resume_state.completed_video_segments.is_empty() {
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&video_tmp_path)
                .await?
        } else {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&video_tmp_path)
                .await?
        };

        let mut audio_file = if resume_state.completed_audio_segments.is_empty() {
            tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&audio_tmp_path)
                .await?
        } else {
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&audio_tmp_path)
                .await?
        };

        let mut downloaded_bytes =
            video_file.metadata().await?.len() + audio_file.metadata().await?.len();
        ctx.progress.report(downloaded_bytes, ctx.job.total_bytes());

        // Download video segments
        for (idx, seg) in video_segments.iter().enumerate() {
            let seg_idx = idx as u32;
            if resume_state.completed_video_segments.contains(&seg_idx) {
                continue;
            }

            if ctx.control.is_cancelled() {
                video_file.flush().await?;
                audio_file.flush().await?;
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                video_file.flush().await?;
                audio_file.flush().await?;
                let state_json = serde_json::to_string(&resume_state)
                    .map_err(|e| BackendError::Media(e.to_string()))?;
                ctx.store.set_backend_state(ctx.job.id(), &state_json)?;
                return Ok(RunOutcome::Paused);
            }

            self.download_segment(&ctx, seg, &mut video_file, &mut downloaded_bytes)
                .await?;

            resume_state.completed_video_segments.push(seg_idx);
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

        // Download audio segments
        for (idx, seg) in audio_segments.iter().enumerate() {
            let seg_idx = idx as u32;
            if resume_state.completed_audio_segments.contains(&seg_idx) {
                continue;
            }

            if ctx.control.is_cancelled() {
                video_file.flush().await?;
                audio_file.flush().await?;
                return Ok(RunOutcome::Cancelled);
            }
            if ctx.control.is_paused() {
                video_file.flush().await?;
                audio_file.flush().await?;
                let state_json = serde_json::to_string(&resume_state)
                    .map_err(|e| BackendError::Media(e.to_string()))?;
                ctx.store.set_backend_state(ctx.job.id(), &state_json)?;
                return Ok(RunOutcome::Paused);
            }

            self.download_segment(&ctx, seg, &mut audio_file, &mut downloaded_bytes)
                .await?;

            resume_state.completed_audio_segments.push(seg_idx);
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

        video_file.flush().await?;
        audio_file.flush().await?;
        drop(video_file);
        drop(audio_file);

        let target_path = ctx.job.target_path();

        if !video_segments.is_empty() && !audio_segments.is_empty() {
            mux::mux_audio_video(&video_tmp_path, &audio_tmp_path, &target_path, &ffmpeg)
                .await
                .map_err(|e| BackendError::Media(format!("ffmpeg mux failed: {e}")))?;
        } else if !video_segments.is_empty() {
            mux::mux_to_mp4(&video_tmp_path, &target_path, &ffmpeg)
                .await
                .map_err(|e| BackendError::Media(format!("ffmpeg mux failed: {e}")))?;
        } else if !audio_segments.is_empty() {
            tokio::fs::rename(&audio_tmp_path, &target_path).await?;
        }

        let _ = tokio::fs::remove_file(&video_tmp_path).await;
        let _ = tokio::fs::remove_file(&audio_tmp_path).await;
        let _ = tokio::fs::remove_file(&part_path).await;

        Ok(RunOutcome::Completed)
    }
}

/// A live manifest has no end, so there is nothing finite to download.
fn reject_live(mpd_type: Option<&str>) -> Result<(), BackendError> {
    if mpd_type == Some("dynamic") {
        return Err(BackendError::Media(
            "live streams are not supported".to_string(),
        ));
    }
    Ok(())
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
    lower.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed")
        || lower.contains("9a04f079-9840-4286-ab92-e65be0885f95")
        || lower.contains("com.widevine")
        || lower.contains("com.microsoft.playready")
}

fn is_drm_uri(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    lower.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed")
        || lower.contains("9a04f079-9840-4286-ab92-e65be0885f95")
        || lower.contains("com.widevine")
        || lower.contains("com.microsoft.playready")
}

fn parse_byte_range(range_str: &str) -> Option<(u64, u64)> {
    let clean = range_str.trim();
    let parts: Vec<&str> = clean.split('-').collect();
    if parts.len() == 2 {
        let start = parts[0].trim().parse::<u64>().ok()?;
        let end = parts[1].trim().parse::<u64>().ok()?;
        Some((start, end))
    } else {
        None
    }
}

fn resolve_template(
    template: &str,
    rep_id: &str,
    number: Option<u64>,
    time: Option<u64>,
    bandwidth: Option<u64>,
) -> String {
    let mut result = template.replace("$RepresentationID$", rep_id);

    if let Some(bw) = bandwidth {
        result = result.replace("$Bandwidth$", &bw.to_string());
    }

    if let Some(num) = number {
        result = result.replace("$Number$", &num.to_string());
        while let Some(start) = result.find("$Number%0") {
            if let Some(end) = result[start..].find("d$") {
                let width_str = &result[start + 9..start + end];
                if let Ok(width) = width_str.parse::<usize>() {
                    let formatted = format!("{:0>width$}", num);
                    result.replace_range(start..start + end + 2, &formatted);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    if let Some(t) = time {
        result = result.replace("$Time$", &t.to_string());
        while let Some(start) = result.find("$Time%0") {
            if let Some(end) = result[start..].find("d$") {
                let width_str = &result[start + 7..start + end];
                if let Ok(width) = width_str.parse::<usize>() {
                    let formatted = format!("{:0>width$}", t);
                    result.replace_range(start..start + end + 2, &formatted);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    result.replace("$$", "$")
}

fn derive_filename(url: &Url, default_ext: &str) -> String {
    let raw = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or("stream");

    let stem = if let Some(stripped) = raw.strip_suffix(".mpd") {
        if stripped.is_empty() {
            "stream"
        } else {
            stripped
        }
    } else {
        raw
    };

    let cleaned = sanitize_filename(stem);
    if cleaned.ends_with(".mp4") {
        cleaned
    } else {
        format!("{cleaned}.{default_ext}")
    }
}

fn sanitize_filename(candidate: &str) -> String {
    odm_core::sanitize_filename(candidate).unwrap_or_else(|| "stream".to_string())
}
