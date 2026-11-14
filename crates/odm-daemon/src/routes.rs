//! REST route handlers for the odm daemon API.
use std::collections::HashMap;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, CategoryDto, ChecksumDto, CreateQueueRequest,
    DownloadDetailDto, DownloadItemDto, ErrorResponse, GrabbedFileDto, GrabberRequest,
    GrabberResponse, HealthResponse, JobOptionsDto, ListCategoriesResponse, ListDownloadsResponse,
    ListQueuesResponse, PluginDto, PluginListResponse, ProbeRequest, ProbeResponse, QueueDto,
    SegmentDto, SetCategoryRequest, SettingsDto, UpdateQueueRequest, UpdateSettingsRequest,
};
use odm_core::grabber::{GrabberConfig, SiteGrabber};
use odm_core::{Backend, Job, JobId, JobOptions, JobState, Source, Store};
use odm_media::url::Url;
use serde::Deserialize;

use crate::state::AppState;

/// What a failing handler returns: a status and the JSON error body.
type ApiError = (StatusCode, Json<ErrorResponse>);

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiError {
    (status, Json(ErrorResponse::new(code, message)))
}

/// The queue called `name`. A missing queue is created; if that fails too,
/// the job goes to the default queue.
fn queue_for(store: &Store, name: &str) -> Result<odm_core::Queue, ApiError> {
    if let Ok(queue) = store.queue_by_name(name) {
        return Ok(queue);
    }
    if let Ok(queue) = store.create_queue(name, 3) {
        return Ok(queue);
    }
    store.queue_by_name("default").map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "queue_error",
            e.to_string(),
        )
    })
}

/// Maps queue ids to queue names, so a list of jobs reads the queues once.
pub fn queue_names(store: &Store) -> HashMap<i64, String> {
    store
        .list_queues()
        .unwrap_or_default()
        .into_iter()
        .map(|queue| (queue.id, queue.name))
        .collect()
}

pub fn job_to_dto(job: &Job, queue_names: &HashMap<i64, String>) -> DownloadItemDto {
    let queue_name = queue_names
        .get(&job.queue_id())
        .cloned()
        .unwrap_or_else(|| "default".to_string());

    let total = job.total_bytes();
    let downloaded = job.downloaded_bytes();
    let progress_pct = match total {
        Some(t) if t > 0 => ((downloaded as f64 / t as f64) * 100.0) as f32,
        _ => 0.0,
    };

    let backend = match job.backend() {
        odm_core::job::BackendKind::Http => "http",
        odm_core::job::BackendKind::Torrent => "torrent",
        odm_core::job::BackendKind::Hls => "hls",
        odm_core::job::BackendKind::Dash => "dash",
        odm_core::job::BackendKind::YtDlp => "yt-dlp",
        odm_core::job::BackendKind::Plugin => "plugin",
    };

    DownloadItemDto {
        id: job.id().0,
        url: match job.source() {
            Source::Url(u) => u.clone(),
            Source::Magnet(m) => m.clone(),
            Source::TorrentFile(p) => p.to_string_lossy().to_string(),
        },
        filename: job.filename().to_string(),
        state: format!("{:?}", job.state()).to_lowercase(),
        total_bytes: total,
        downloaded_bytes: downloaded,
        progress_pct,
        speed_bps: 0,
        queue: queue_name,
        target_dir: job.target_dir().to_string_lossy().to_string(),
        category: job.category().map(|s| s.to_string()),
        error: job.error().map(|e| e.message.clone()),
        backend: Some(backend.to_string()),
    }
}

pub fn job_to_detail_dto(job: &Job, store: &Store) -> DownloadDetailDto {
    let item = job_to_dto(job, &queue_names(store));
    let backend = item.backend.clone();
    let segments = store
        .list_segments(job.id())
        .unwrap_or_default()
        .into_iter()
        .map(|s| SegmentDto {
            index: s.index,
            start: s.start,
            end: s.end,
            downloaded: s.downloaded,
            is_complete: s.is_done(),
        })
        .collect();

    let options = JobOptionsDto {
        segments: job.options().segments,
        speed_limit: job.options().speed_limit,
        headers: None,
        user_agent: job.options().user_agent.clone(),
        referer: job.options().referer.clone(),
        cookies: job.options().cookies.clone(),
        checksum: job.options().checksum.as_ref().map(|c| ChecksumDto {
            algorithm: match c.algorithm {
                odm_core::checksum::ChecksumAlgorithm::Sha256 => odm_api::ChecksumAlgorithm::Sha256,
                odm_core::checksum::ChecksumAlgorithm::Sha1 => odm_api::ChecksumAlgorithm::Sha1,
                odm_core::checksum::ChecksumAlgorithm::Md5 => odm_api::ChecksumAlgorithm::Md5,
            },
            digest: c.expected.clone(),
        }),
        mirrors: if job.options().mirrors.is_empty() {
            None
        } else {
            Some(job.options().mirrors.clone())
        },
        selected_files: job.options().selected_files.clone(),
        seed_ratio: job.options().seed_ratio,
        quality: job.options().quality.clone(),
    };

    DownloadDetailDto {
        item,
        segments,
        options,
        backend,
    }
}

pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".to_string(),
    })
}

pub async fn probe_handler(
    State(state): State<AppState>,
    Json(req): Json<ProbeRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.url.starts_with("magnet:") || req.url.ends_with(".torrent") {
        let torrent = state.torrent.as_ref().ok_or_else(|| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "torrent_disabled",
                "Torrent backend is not configured",
            )
        })?;

        let source = if req.url.starts_with("magnet:") {
            Source::Magnet(req.url.clone())
        } else {
            Source::TorrentFile(PathBuf::from(&req.url))
        };

        let probe = torrent
            .probe(&source, &JobOptions::default())
            .await
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, "probe_failed", e.to_string()))?;

        let files = torrent.probe_torrent_files(&source).await.ok();
        let torrent_files = files.map(|fs| {
            fs.into_iter()
                .map(|f| odm_api::TorrentFileDto {
                    index: f.index,
                    path: f.path,
                    size: f.size,
                })
                .collect()
        });

        return Ok(Json(ProbeResponse {
            backend: "torrent".to_string(),
            filename: probe.filename,
            size: probe.total_bytes,
            ranges: probe.supports_ranges,
            etag: probe.etag,
            last_modified: probe.last_modified,
            torrent_files,
            qualities: None,
        }));
    }

    let is_plugin = {
        let plugins = state.plugins.read().await;
        plugins.find_for_url(&req.url).is_some()
    };

    if is_plugin {
        let plugin = state
            .engine
            .get_backend(odm_core::job::BackendKind::Plugin)
            .map_err(|e| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "backend_error",
                    e.to_string(),
                )
            })?;

        let source = Source::Url(req.url.clone());
        let opts = JobOptions {
            user_agent: req.user_agent.clone(),
            ..Default::default()
        };

        let probe = plugin
            .probe(&source, &opts)
            .await
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, "probe_failed", e.to_string()))?;

        return Ok(Json(ProbeResponse {
            backend: "plugin".to_string(),
            filename: probe.filename,
            size: probe.total_bytes,
            ranges: probe.supports_ranges,
            etag: probe.etag,
            last_modified: probe.last_modified,
            torrent_files: None,
            qualities: None,
        }));
    }

    let parsed_url = odm_media::url::Url::parse(&req.url).ok();
    let is_m3u8 = parsed_url
        .as_ref()
        .and_then(|u| u.path().rsplit('.').next())
        .map(|ext| ext.eq_ignore_ascii_case("m3u8"))
        .unwrap_or_else(|| req.url.ends_with(".m3u8") || req.url.contains(".m3u8?"));

    let is_mpd = parsed_url
        .as_ref()
        .and_then(|u| u.path().rsplit('.').next())
        .map(|ext| ext.eq_ignore_ascii_case("mpd"))
        .unwrap_or_else(|| req.url.ends_with(".mpd") || req.url.contains(".mpd?"));

    if is_m3u8 {
        let hls = state.hls.as_ref().ok_or_else(|| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "hls_disabled",
                "HLS backend is not configured",
            )
        })?;

        let source = Source::Url(req.url.clone());
        let opts = JobOptions {
            user_agent: req.user_agent,
            ..Default::default()
        };

        let probe = hls
            .probe(&source, &opts)
            .await
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, "probe_failed", e.to_string()))?;

        let qualities = hls.probe_qualities(&source).await.ok().map(|qs| {
            qs.into_iter()
                .map(|q| odm_api::QualityDto {
                    id: q.id,
                    resolution: q.resolution,
                    bandwidth: q.bandwidth,
                    codecs: q.codecs,
                    label: q.label,
                })
                .collect()
        });

        return Ok(Json(ProbeResponse {
            backend: "hls".to_string(),
            filename: probe.filename,
            size: probe.total_bytes,
            ranges: probe.supports_ranges,
            etag: probe.etag,
            last_modified: probe.last_modified,
            torrent_files: None,
            qualities,
        }));
    }

    if is_mpd {
        let dash = state.dash.as_ref().ok_or_else(|| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dash_disabled",
                "DASH backend is not configured",
            )
        })?;

        let source = Source::Url(req.url.clone());
        let opts = JobOptions {
            user_agent: req.user_agent,
            ..Default::default()
        };

        let probe = dash
            .probe(&source, &opts)
            .await
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, "probe_failed", e.to_string()))?;

        let qualities = dash.probe_qualities(&source).await.ok().map(|qs| {
            qs.into_iter()
                .map(|q| odm_api::QualityDto {
                    id: q.id,
                    resolution: q.resolution,
                    bandwidth: q.bandwidth,
                    codecs: q.codecs,
                    label: q.label,
                })
                .collect()
        });

        return Ok(Json(ProbeResponse {
            backend: "dash".to_string(),
            filename: probe.filename,
            size: probe.total_bytes,
            ranges: probe.supports_ranges,
            etag: probe.etag,
            last_modified: probe.last_modified,
            torrent_files: None,
            qualities,
        }));
    }

    // The engine's own HTTP backend keeps one connection pool for every probe.
    let backend = state
        .engine
        .get_backend(odm_core::job::BackendKind::Http)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "backend_error",
                e.to_string(),
            )
        })?;

    let opts = JobOptions {
        user_agent: req.user_agent,
        ..Default::default()
    };

    let probe = backend
        .probe(&Source::Url(req.url), &opts)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "probe_failed", e.to_string()))?;

    Ok(Json(ProbeResponse {
        backend: "http".to_string(),
        filename: probe.filename,
        size: probe.total_bytes,
        ranges: probe.supports_ranges,
        etag: probe.etag,
        last_modified: probe.last_modified,
        torrent_files: None,
        qualities: None,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListFilter {
    pub state: Option<String>,
    pub queue: Option<String>,
    pub category: Option<String>,
}

pub async fn list_downloads_handler(
    State(state): State<AppState>,
    Query(filter): Query<ListFilter>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();
    let jobs = store.list_jobs().map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            e.to_string(),
        )
    })?;

    let names = queue_names(store);
    let downloads: Vec<DownloadItemDto> = jobs
        .into_iter()
        .map(|job| job_to_dto(&job, &names))
        .filter(|dto| {
            if let Some(ref st) = filter.state
                && &dto.state != st
            {
                return false;
            }
            if let Some(ref q) = filter.queue
                && &dto.queue != q
            {
                return false;
            }
            if let Some(ref cat) = filter.category
                && dto.category.as_ref() != Some(cat)
            {
                return false;
            }
            true
        })
        .collect();

    Ok(Json(ListDownloadsResponse { downloads }))
}

pub async fn add_download_handler(
    State(state): State<AppState>,
    Json(req): Json<AddDownloadRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();

    let forced_backend = match req.backend.as_deref() {
        None => None,
        Some("ytdlp") if state.ytdlp.is_some() => Some(odm_core::job::BackendKind::YtDlp),
        Some("ytdlp") => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "backend_unavailable",
                "yt-dlp was not found; install it or set ytdlp_path in the settings",
            ));
        }
        Some(other) => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "unknown_backend",
                format!("unknown backend '{other}'"),
            ));
        }
    };

    let (target_dir, category_name) = if let Some(cat_name) = req.category {
        let cat = store.get_category(&cat_name).ok();
        let cat_dir = cat.and_then(|c| {
            if c.directory.is_empty() {
                None
            } else {
                Some(PathBuf::from(c.directory))
            }
        });
        let dir = if let Some(dir) = req.target_dir {
            PathBuf::from(dir)
        } else if let Some(cd) = cat_dir {
            cd
        } else {
            let base_dir = if let Ok(Some(d)) = store.get_setting("download_dir") {
                PathBuf::from(d)
            } else {
                directories::UserDirs::new()
                    .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("./downloads"))
            };
            base_dir.join(&cat_name)
        };
        (dir, Some(cat_name))
    } else if let Some(dir) = req.target_dir {
        (PathBuf::from(dir), None)
    } else {
        let candidate_filename = req
            .filename
            .as_deref()
            .or_else(|| req.url.rsplit_once('/').map(|(_, f)| f))
            .unwrap_or("");
        let cat = store.category_for_filename(candidate_filename).ok();
        let cat_name = cat.as_ref().map(|c| c.name.clone());
        let cat_dir = cat.and_then(|c| {
            if c.directory.is_empty() {
                None
            } else {
                Some(PathBuf::from(c.directory))
            }
        });

        let base_dir = if let Ok(Some(d)) = store.get_setting("download_dir") {
            PathBuf::from(d)
        } else {
            directories::UserDirs::new()
                .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("./downloads"))
        };

        let dir = match cat_dir {
            Some(cd) => cd,
            None => {
                if let Some(ref cn) = cat_name
                    && cn != "Other"
                {
                    base_dir.join(cn)
                } else {
                    base_dir
                }
            }
        };
        (dir, cat_name)
    };

    let mut options = JobOptions::default();
    if let Some(ref opts) = req.options {
        options.segments = opts.segments;
        options.speed_limit = opts.speed_limit;
        if let Some(ref headers) = opts.headers {
            options.headers = headers.clone();
        }
        if let Some(ref ua) = opts.user_agent {
            options.user_agent = Some(ua.clone());
        }
        if let Some(ref r) = opts.referer {
            options.referer = Some(r.clone());
        }
        if let Some(ref c) = opts.cookies {
            options.cookies = Some(c.clone());
        }
        if let Some(ref cs) = opts.checksum {
            options.checksum = Some(odm_core::checksum::Checksum {
                algorithm: match cs.algorithm {
                    odm_api::ChecksumAlgorithm::Sha256 => {
                        odm_core::checksum::ChecksumAlgorithm::Sha256
                    }
                    odm_api::ChecksumAlgorithm::Sha1 => odm_core::checksum::ChecksumAlgorithm::Sha1,
                    odm_api::ChecksumAlgorithm::Md5 => odm_core::checksum::ChecksumAlgorithm::Md5,
                },
                expected: cs.digest.clone(),
            });
        }
        if let Some(ref m) = opts.mirrors {
            options.mirrors = m.clone();
        }
        if let Some(ref sf) = opts.selected_files {
            options.selected_files = Some(sf.clone());
        }
        if let Some(sr) = opts.seed_ratio {
            options.seed_ratio = Some(sr);
        }
        if let Some(ref q) = opts.quality {
            options.quality = Some(q.clone());
        }
    }

    // Settings fill in what the request left open.
    let stored_setting = |key: &str| -> Option<u32> {
        store
            .get_setting(key)
            .ok()
            .flatten()
            .and_then(|value| value.parse().ok())
    };
    if options.segments.is_none() {
        options.segments = stored_setting("default_segments");
    }
    if options.retry_count.is_none() {
        options.retry_count = stored_setting("default_retry_count");
    }

    let queue_name = req.queue.unwrap_or_else(|| "default".to_string());
    let queue = queue_for(store, &queue_name)?;

    let is_web_url = req.url.starts_with("http://") || req.url.starts_with("https://");
    let source = if forced_backend.is_some() || is_web_url {
        Source::Url(req.url.clone())
    } else if req.url.starts_with("magnet:") {
        Source::Magnet(req.url.clone())
    } else if req.url.ends_with(".torrent") || std::path::Path::new(&req.url).is_file() {
        Source::TorrentFile(PathBuf::from(&req.url))
    } else {
        Source::Url(req.url.clone())
    };

    let is_torrent = matches!(source, Source::Magnet(_) | Source::TorrentFile(_));
    let is_plugin = if !is_torrent {
        let plugins = state.plugins.read().await;
        plugins.find_for_url(&req.url).is_some()
    } else {
        false
    };

    let overrides = odm_core::AddOverrides {
        filename: req.filename,
        category: category_name,
        queue_id: Some(queue.id),
        backend: forced_backend.or(is_plugin.then_some(odm_core::job::BackendKind::Plugin)),
    };
    let job = state
        .engine
        .add_download_with(source, target_dir, options, overrides)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "add_failed", e.to_string()))?;

    let id = job.id();
    state.spawn_download(id, &queue.name, queue.max_concurrent);

    Ok(Json(AddDownloadResponse { id: id.0 }))
}

pub async fn get_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();
    let job = store
        .get_job(JobId(id))
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "not_found", "Job not found"))?;

    Ok(Json(job_to_detail_dto(&job, store)))
}

pub async fn pause_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .engine
        .pause(JobId(id))
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "pause_failed", e.to_string()))?;
    Ok(StatusCode::OK)
}

pub async fn resume_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    restart_download(&state, id, &[JobState::Paused, JobState::Queued])
}

pub async fn retry_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    restart_download(&state, id, &[JobState::Failed])
}

/// Starts a job again if it is in one of the `allowed` states, otherwise answers 409.
fn restart_download(
    state: &AppState,
    id: i64,
    allowed: &[JobState],
) -> Result<StatusCode, ApiError> {
    let store = state.engine.store();
    let job = store
        .get_job(JobId(id))
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "not_found", "Job not found"))?;
    if !allowed.contains(&job.state()) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "invalid_state",
            format!("A {:?} download cannot be restarted this way", job.state()),
        ));
    }
    let queue = store
        .list_queues()
        .ok()
        .and_then(|qs| qs.into_iter().find(|q| q.id == job.queue_id()))
        .unwrap_or_else(|| {
            store
                .queue_by_name("default")
                .unwrap_or_else(|_| odm_core::Queue {
                    id: 1,
                    name: "default".to_string(),
                    max_concurrent: 3,
                    schedule: None,
                })
        });

    state.spawn_download(JobId(id), &queue.name, queue.max_concurrent);
    Ok(StatusCode::OK)
}

pub async fn cancel_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .engine
        .cancel(JobId(id))
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "cancel_failed", e.to_string()))?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize, Default)]
pub struct DeleteQuery {
    #[serde(default)]
    pub delete_file: bool,
}

pub async fn delete_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<DeleteQuery>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .engine
        .remove(JobId(id), query.delete_file)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "remove_failed", e.to_string()))?;
    Ok(StatusCode::OK)
}

fn queue_to_dto(q: odm_core::Queue) -> QueueDto {
    QueueDto {
        id: q.id,
        name: q.name,
        max_concurrent: q.max_concurrent,
        schedule_start: q.schedule.as_ref().map(|s| s.start_time.clone()),
        schedule_stop: q.schedule.as_ref().and_then(|s| s.stop_time.clone()),
        schedule_days: q.schedule.map(|s| s.days),
    }
}

pub async fn list_queues_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let queues = state.engine.store().list_queues().map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            e.to_string(),
        )
    })?;

    let dtos = queues.into_iter().map(queue_to_dto).collect();

    Ok(Json(ListQueuesResponse { queues: dtos }))
}

pub async fn create_queue_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateQueueRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let sched = req.schedule_start.map(|start| {
        odm_core::schedule::QueueSchedule::new(
            start,
            req.schedule_stop,
            req.schedule_days.unwrap_or_default(),
        )
    });
    let queue = state
        .engine
        .store()
        .create_queue_with_schedule(&req.name, req.max_concurrent, sched.as_ref())
        .map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                "create_queue_failed",
                e.to_string(),
            )
        })?;

    Ok(Json(queue_to_dto(queue)))
}

pub async fn update_queue_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();
    let sched_opt = if req.schedule_start.is_some() {
        Some(req.schedule_start.map(|start| {
            odm_core::schedule::QueueSchedule::new(
                start,
                req.schedule_stop,
                req.schedule_days.unwrap_or_default(),
            )
        }))
    } else {
        None
    };

    let updated = store
        .update_queue_schedule(&name, Some(req.max_concurrent), sched_opt)
        .map_err(|e| {
            api_error(
                StatusCode::BAD_REQUEST,
                "update_queue_failed",
                e.to_string(),
            )
        })?;
    state.set_queue_limit(&name, updated.max_concurrent);
    state.engine.announce_queue_changed(&name);

    Ok(Json(queue_to_dto(updated)))
}

pub async fn delete_queue_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state.engine.store().delete_queue(&name).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            "delete_queue_failed",
            e.to_string(),
        )
    })?;
    state.engine.announce_queue_changed(&name);
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct PositionRequest {
    pub position: i64,
}

pub async fn set_position_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(req): Json<PositionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .engine
        .store()
        .get_job(JobId(id))
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "not_found", "Job not found"))?;
    state
        .engine
        .set_job_position(JobId(id), req.position)
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "position_failed",
                e.to_string(),
            )
        })?;
    Ok(StatusCode::OK)
}

pub async fn get_settings_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();
    let download_dir = store.get_setting("download_dir").ok().flatten();
    let default_segments = store
        .get_setting("default_segments")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let global_speed_limit = store
        .get_setting("global_speed_limit")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let default_retry_count = store
        .get_setting("default_retry_count")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let seed_ratio = store
        .get_setting("seed_ratio")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(odm_torrent::DEFAULT_SEED_RATIO);
    let torrent_listen_port = store
        .get_setting("torrent_listen_port")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let ffmpeg_path = store
        .get_setting("ffmpeg_path")
        .ok()
        .flatten()
        .unwrap_or_default();
    let ytdlp_path = store
        .get_setting("ytdlp_path")
        .ok()
        .flatten()
        .unwrap_or_default();
    let allowed_hosts = store
        .get_setting("allowed_hosts")
        .ok()
        .flatten()
        .unwrap_or_default();

    Ok(Json(SettingsDto {
        download_dir,
        default_segments,
        global_speed_limit,
        default_retry_count,
        seed_ratio,
        torrent_listen_port,
        ffmpeg_path,
        ytdlp_path,
        allowed_hosts,
    }))
}

pub async fn update_settings_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let store = state.engine.store();
    if let Some(ref dir) = req.download_dir {
        let _ = store.set_setting("download_dir", dir);
    }
    if let Some(s) = req.default_segments {
        let _ = store.set_setting("default_segments", &s.to_string());
    }
    if let Some(l) = req.global_speed_limit {
        let _ = store.set_setting("global_speed_limit", &l.to_string());
    }
    if let Some(r) = req.default_retry_count {
        let _ = store.set_setting("default_retry_count", &r.to_string());
    }
    if let Some(sr) = req.seed_ratio {
        let _ = store.set_setting("seed_ratio", &sr.to_string());
    }
    if let Some(port) = req.torrent_listen_port {
        let _ = store.set_setting("torrent_listen_port", &port.to_string());
    }
    if let Some(ref p) = req.ffmpeg_path {
        let _ = store.set_setting("ffmpeg_path", p);
    }
    if let Some(ref p) = req.ytdlp_path {
        let _ = store.set_setting("ytdlp_path", p);
    }
    if let Some(ref hosts) = req.allowed_hosts {
        let _ = store.set_setting("allowed_hosts", hosts);
    }
    state.apply_global_speed_limit();
    state.reload_allowed_hosts();

    get_settings_handler(State(state)).await
}

pub async fn list_categories_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let categories = state.engine.store().list_categories().map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_error",
            e.to_string(),
        )
    })?;

    let dtos: Vec<CategoryDto> = categories
        .into_iter()
        .map(|c| CategoryDto {
            name: c.name,
            directory: c.directory,
            extensions: c.extensions,
        })
        .collect();

    Ok(Json(ListCategoriesResponse { categories: dtos }))
}

pub async fn set_category_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SetCategoryRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let cat = state
        .engine
        .store()
        .set_category(&name, req.directory.as_deref(), req.extensions.as_deref())
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_error",
                e.to_string(),
            )
        })?;

    Ok(Json(CategoryDto {
        name: cat.name,
        directory: cat.directory,
        extensions: cat.extensions,
    }))
}

pub async fn delete_category_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state.engine.store().delete_category(&name).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            "delete_category_failed",
            e.to_string(),
        )
    })?;

    Ok(StatusCode::OK)
}

pub async fn get_plugins_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let plugins = state.plugins.read().await.list();
    let dtos: Vec<PluginDto> = plugins
        .into_iter()
        .map(|p| PluginDto {
            name: p.name,
            version: p.version,
            description: p.description,
            role: p.role,
            url_patterns: p.url_patterns,
            command: p.command,
        })
        .collect();

    Ok(Json(PluginListResponse { plugins: dtos }))
}

pub async fn grabber_handler(
    State(state): State<AppState>,
    Json(req): Json<GrabberRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let start_url = Url::parse(&req.url).map_err(|e| {
        api_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            format!("invalid start url: {e}"),
        )
    })?;

    let config = GrabberConfig {
        start_url,
        depth_limit: req.depth.unwrap_or(1),
        extensions: req.extensions.unwrap_or_default(),
        pattern: req.pattern,
        respect_robots_txt: req.respect_robots.unwrap_or(true),
        max_pages: req.max_pages.unwrap_or(50),
    };

    let grabber = SiteGrabber::new(config);
    let result = grabber
        .crawl(&state.http_client)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, "grabber_failed", e.to_string()))?;

    let files = result
        .files
        .into_iter()
        .map(|f| GrabbedFileDto {
            url: f.url.to_string(),
            filename: f.filename,
            found_on: f.found_on.to_string(),
            depth: f.depth,
        })
        .collect();

    Ok(Json(GrabberResponse {
        files,
        pages_crawled: result.pages_crawled,
    }))
}
