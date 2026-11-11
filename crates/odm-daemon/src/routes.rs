//! REST route handlers for the odm daemon API.
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, ChecksumDto, CreateQueueRequest, DownloadDetailDto,
    DownloadItemDto, ErrorResponse, HealthResponse, JobOptionsDto, ListDownloadsResponse,
    ListQueuesResponse, ProbeRequest, ProbeResponse, QueueDto, SegmentDto, SettingsDto,
    UpdateQueueRequest, UpdateSettingsRequest,
};
use odm_core::{Backend, HttpBackend, Job, JobId, JobOptions, Source, Store};
use serde::Deserialize;

use crate::state::AppState;

pub fn job_to_dto(job: &Job, store: &Store) -> DownloadItemDto {
    let queue_name = store
        .list_queues()
        .ok()
        .and_then(|qs| qs.into_iter().find(|q| q.id == job.queue_id()))
        .map(|q| q.name)
        .unwrap_or_else(|| "default".to_string());

    let total = job.total_bytes();
    let downloaded = job.downloaded_bytes();
    let progress_pct = match total {
        Some(t) if t > 0 => ((downloaded as f64 / t as f64) * 100.0) as f32,
        _ => 0.0,
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
        error: job.error().map(|e| e.message.clone()),
    }
}

pub fn job_to_detail_dto(job: &Job, store: &Store) -> DownloadDetailDto {
    let item = job_to_dto(job, store);
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
    };

    DownloadDetailDto {
        item,
        segments,
        options,
    }
}

pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status: "ok".to_string(),
    })
}

pub async fn probe_handler(
    Json(req): Json<ProbeRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let backend = HttpBackend::new().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("backend_error", e.to_string())),
        )
    })?;

    let opts = JobOptions {
        user_agent: req.user_agent,
        ..Default::default()
    };

    let probe = backend
        .probe(&Source::Url(req.url), &opts)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("probe_failed", e.to_string())),
            )
        })?;

    Ok(Json(ProbeResponse {
        backend: "http".to_string(),
        filename: probe.filename,
        size: probe.total_bytes,
        ranges: probe.supports_ranges,
        etag: probe.etag,
        last_modified: probe.last_modified,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct ListFilter {
    pub state: Option<String>,
    pub queue: Option<String>,
}

pub async fn list_downloads_handler(
    State(state): State<AppState>,
    Query(filter): Query<ListFilter>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();
    let jobs = store.list_jobs().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("store_error", e.to_string())),
        )
    })?;

    let downloads: Vec<DownloadItemDto> = jobs
        .into_iter()
        .map(|job| job_to_dto(&job, store))
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
            true
        })
        .collect();

    Ok(Json(ListDownloadsResponse { downloads }))
}

pub async fn add_download_handler(
    State(state): State<AppState>,
    Json(req): Json<AddDownloadRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();

    let target_dir = if let Some(dir) = req.target_dir {
        PathBuf::from(dir)
    } else if let Ok(Some(d)) = store.get_setting("download_dir") {
        PathBuf::from(d)
    } else {
        directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("./downloads"))
    };

    let mut options = JobOptions::default();
    if let Some(ref opts) = req.options {
        options.segments = opts.segments;
        options.speed_limit = opts.speed_limit;
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
    }

    let queue_name = req.queue.unwrap_or_else(|| "default".to_string());
    let queue = match store.queue_by_name(&queue_name) {
        Ok(q) => q,
        Err(_) => match store.create_queue(&queue_name, 3) {
            Ok(q) => q,
            Err(_) => store.queue_by_name("default").map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("queue_error", e.to_string())),
                )
            })?,
        },
    };

    let mut job = state
        .engine
        .add(&req.url, target_dir, options)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("add_failed", e.to_string())),
            )
        })?;

    if let Some(fname) = req.filename {
        job.set_filename(fname);
    }
    job.set_queue_id(queue.id);
    let _ = store.update_job(&job);

    let id = job.id();
    state.spawn_download(id, &queue.name, queue.max_concurrent);

    Ok(Json(AddDownloadResponse { id: id.0 }))
}

pub async fn get_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();
    let job = store.get_job(JobId(id)).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("not_found", "Job not found")),
        )
    })?;

    Ok(Json(job_to_detail_dto(&job, store)))
}

pub async fn pause_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state.engine.pause(JobId(id)).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("pause_failed", e.to_string())),
        )
    })?;
    Ok(StatusCode::OK)
}

pub async fn resume_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();
    let job = store.get_job(JobId(id)).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("not_found", "Job not found")),
        )
    })?;
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
                })
        });

    state.spawn_download(JobId(id), &queue.name, queue.max_concurrent);
    Ok(StatusCode::OK)
}

pub async fn cancel_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state.engine.cancel(JobId(id)).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("cancel_failed", e.to_string())),
        )
    })?;
    Ok(StatusCode::OK)
}

pub async fn retry_download_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();
    let job = store.get_job(JobId(id)).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("not_found", "Job not found")),
        )
    })?;
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
                })
        });

    state.spawn_download(JobId(id), &queue.name, queue.max_concurrent);
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
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state
        .engine
        .remove(JobId(id), query.delete_file)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("remove_failed", e.to_string())),
            )
        })?;
    Ok(StatusCode::OK)
}

pub async fn list_queues_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let queues = state.engine.store().list_queues().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new("store_error", e.to_string())),
        )
    })?;

    let dtos = queues
        .into_iter()
        .map(|q| QueueDto {
            id: q.id,
            name: q.name,
            max_concurrent: q.max_concurrent,
        })
        .collect();

    Ok(Json(ListQueuesResponse { queues: dtos }))
}

pub async fn create_queue_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateQueueRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let queue = state
        .engine
        .store()
        .create_queue(&req.name, req.max_concurrent)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("create_queue_failed", e.to_string())),
            )
        })?;

    Ok(Json(QueueDto {
        id: queue.id,
        name: queue.name,
        max_concurrent: queue.max_concurrent,
    }))
}

pub async fn update_queue_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateQueueRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let store = state.engine.store();
    let queue = store.queue_by_name(&name).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("not_found", "Queue not found")),
        )
    })?;

    store
        .set_queue_limit(queue.id, req.max_concurrent)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new("update_queue_failed", e.to_string())),
            )
        })?;

    let updated = store.queue_by_name(&name).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("not_found", "Queue not found")),
        )
    })?;

    Ok(Json(QueueDto {
        id: updated.id,
        name: updated.name,
        max_concurrent: updated.max_concurrent,
    }))
}

pub async fn delete_queue_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    state.engine.store().delete_queue(&name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("delete_queue_failed", e.to_string())),
        )
    })?;
    Ok(StatusCode::OK)
}

pub async fn get_settings_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
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

    Ok(Json(SettingsDto {
        download_dir,
        default_segments,
        global_speed_limit,
        default_retry_count,
    }))
}

pub async fn update_settings_handler(
    State(state): State<AppState>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
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

    get_settings_handler(State(state)).await
}
