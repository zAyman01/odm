//! Torrent backend for odm, built on librqbit.
#![forbid(unsafe_code)]

pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ListenerOptions, Session, SessionOptions,
};
use odm_core::backend::{Backend, BackendError, JobContext, RunOutcome};
use odm_core::job::{BackendKind, JobOptions, JobState, ProbeResult, Source};
pub use state::TorrentResumeState;

/// Information about a single file in a torrent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TorrentFileInfo {
    pub index: u32,
    pub path: String,
    pub size: u64,
}

/// BitTorrent backend powered by librqbit.
pub struct TorrentBackend {
    session: Arc<Session>,
    default_seed_ratio: f64,
}

impl TorrentBackend {
    /// Creates a new `TorrentBackend` instance.
    pub async fn new(download_dir: PathBuf, listen_port: u16) -> Result<Self, BackendError> {
        let listen = if listen_port > 0 {
            Some(ListenerOptions {
                listen_addr: SocketAddr::from(([0, 0, 0, 0], listen_port)),
                ..Default::default()
            })
        } else {
            None
        };

        let opts = SessionOptions {
            listen,
            ..Default::default()
        };

        let session = Session::new_with_opts(download_dir, opts)
            .await
            .map_err(|e| BackendError::Torrent(e.to_string()))?;

        Ok(Self {
            session,
            default_seed_ratio: 0.0,
        })
    }

    /// Creates a `TorrentBackend` wrapping an existing `Session`.
    pub fn new_with_session(session: Arc<Session>, default_seed_ratio: f64) -> Self {
        Self {
            session,
            default_seed_ratio,
        }
    }

    /// Returns the underlying `librqbit::Session`.
    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    /// Returns the default seed ratio.
    pub fn default_seed_ratio(&self) -> f64 {
        self.default_seed_ratio
    }

    /// Sets the default seed ratio.
    pub fn set_default_seed_ratio(&mut self, ratio: f64) {
        self.default_seed_ratio = ratio;
    }

    /// Probes the list of files inside a torrent without initiating a download.
    pub async fn probe_torrent_files(
        &self,
        source: &Source,
    ) -> Result<Vec<TorrentFileInfo>, BackendError> {
        match source {
            Source::TorrentFile(path) => {
                let bytes = tokio::fs::read(path).await.map_err(BackendError::Io)?;
                let torrent = librqbit::torrent_from_bytes(&bytes)
                    .map_err(|e| BackendError::Torrent(format!("invalid torrent file: {e}")))?;

                let mut files = Vec::new();
                if let Some(meta_files) = &torrent.info.data.files {
                    for (idx, f) in meta_files.iter().enumerate() {
                        let path_str = f
                            .path
                            .iter()
                            .map(|p| String::from_utf8_lossy(p.as_ref()).into_owned())
                            .collect::<Vec<_>>()
                            .join("/");
                        files.push(TorrentFileInfo {
                            index: idx as u32,
                            path: path_str,
                            size: f.length,
                        });
                    }
                } else if let Some(len) = torrent.info.data.length {
                    let name = torrent
                        .info
                        .data
                        .name
                        .as_ref()
                        .map(|n| String::from_utf8_lossy(n.as_ref()).into_owned())
                        .unwrap_or_else(|| "file".to_string());
                    files.push(TorrentFileInfo {
                        index: 0,
                        path: name,
                        size: len,
                    });
                }
                Ok(files)
            }
            Source::Magnet(uri) => {
                let peers = extract_peers_from_magnet(uri);
                let add_opts = AddTorrentOptions {
                    paused: false,
                    initial_peers: if peers.is_empty() { None } else { Some(peers) },
                    overwrite: true,
                    ..Default::default()
                };

                let resp = self
                    .session
                    .add_torrent(AddTorrent::from_url(uri.as_str()), Some(add_opts))
                    .await
                    .map_err(|e| BackendError::Torrent(e.to_string()))?;

                let (_id, handle) = match resp {
                    AddTorrentResponse::Added(id, h) => (id, h),
                    AddTorrentResponse::AlreadyManaged(id, h) => (id, h),
                    AddTorrentResponse::ListOnly(_) => {
                        return Err(BackendError::Torrent("unexpected list_only".into()));
                    }
                };

                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    handle.wait_until_initialized(),
                )
                .await;

                let files = handle
                    .with_metadata(|meta| {
                        let mut list = Vec::new();
                        for (idx, detail) in meta.info.iter_file_details().enumerate() {
                            list.push(TorrentFileInfo {
                                index: idx as u32,
                                path: detail.filename.to_string(),
                                size: detail.len,
                            });
                        }
                        list
                    })
                    .map_err(|e| BackendError::Torrent(e.to_string()))?;

                let _ = self.session.pause(&handle).await;
                Ok(files)
            }
            Source::Url(_) => Err(BackendError::Unsupported),
        }
    }
}

fn extract_peers_from_magnet(uri: &str) -> Vec<SocketAddr> {
    let mut peers = Vec::new();
    if let Some(query) = uri.split_once('?').map(|(_, q)| q) {
        for part in query.split('&') {
            if let Some(addr) = part
                .strip_prefix("x.pe=")
                .and_then(|val| val.parse::<SocketAddr>().ok())
            {
                peers.push(addr);
            }
        }
    }
    peers
}

#[async_trait]
impl Backend for TorrentBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Torrent
    }

    async fn probe(
        &self,
        source: &Source,
        _opts: &JobOptions,
    ) -> Result<ProbeResult, BackendError> {
        match source {
            Source::TorrentFile(path) => {
                let bytes = tokio::fs::read(path).await.map_err(BackendError::Io)?;
                let torrent = librqbit::torrent_from_bytes(&bytes)
                    .map_err(|e| BackendError::Torrent(format!("invalid torrent file: {e}")))?;

                let filename = torrent
                    .info
                    .data
                    .name
                    .as_ref()
                    .map(|n| String::from_utf8_lossy(n.as_ref()).into_owned())
                    .unwrap_or_else(|| "torrent".to_string());

                let total_bytes = torrent.info.data.length.or_else(|| {
                    torrent
                        .info
                        .data
                        .files
                        .as_ref()
                        .map(|files| files.iter().map(|f| f.length).sum())
                });

                Ok(ProbeResult {
                    backend: BackendKind::Torrent,
                    filename,
                    total_bytes,
                    supports_ranges: true,
                    etag: None,
                    last_modified: None,
                    content_type: Some("application/x-bittorrent".to_string()),
                })
            }
            Source::Magnet(uri) => {
                let magnet = librqbit::Magnet::parse(uri)
                    .map_err(|e| BackendError::InvalidUrl(format!("invalid magnet URI: {e}")))?;

                let peers = extract_peers_from_magnet(uri);
                let add_opts = AddTorrentOptions {
                    paused: false,
                    initial_peers: if peers.is_empty() { None } else { Some(peers) },
                    overwrite: true,
                    ..Default::default()
                };

                let resp = self
                    .session
                    .add_torrent(AddTorrent::from_url(uri.as_str()), Some(add_opts))
                    .await
                    .map_err(|e| BackendError::Torrent(e.to_string()))?;

                let (_id, handle) = match resp {
                    AddTorrentResponse::Added(id, h) => (id, h),
                    AddTorrentResponse::AlreadyManaged(id, h) => (id, h),
                    AddTorrentResponse::ListOnly(_) => {
                        return Err(BackendError::Torrent("unexpected list_only".into()));
                    }
                };

                let init_result = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    handle.wait_until_initialized(),
                )
                .await;

                let (filename, total_bytes) = if init_result.is_ok() {
                    handle
                        .with_metadata(|meta| {
                            let name = meta
                                .info
                                .name()
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "torrent".to_string());
                            let total = meta.info.lengths().total_length();
                            (name, Some(total))
                        })
                        .map_err(|e| BackendError::Torrent(e.to_string()))?
                } else {
                    let name = magnet.name.unwrap_or_else(|| "torrent".to_string());
                    (name, None)
                };

                let _ = self.session.pause(&handle).await;

                Ok(ProbeResult {
                    backend: BackendKind::Torrent,
                    filename,
                    total_bytes,
                    supports_ranges: true,
                    etag: None,
                    last_modified: None,
                    content_type: Some("application/x-bittorrent".to_string()),
                })
            }
            Source::Url(_) => Err(BackendError::Unsupported),
        }
    }

    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError> {
        if ctx.control.is_cancelled() {
            return Ok(RunOutcome::Cancelled);
        }
        if ctx.control.is_paused() {
            return Ok(RunOutcome::Paused);
        }

        let resume_state: Option<TorrentResumeState> =
            match ctx.store.get_backend_state(ctx.job.id())? {
                Some(json_str) => serde_json::from_str(&json_str).ok(),
                None => None,
            };

        let existing_handle = resume_state.and_then(|st| {
            self.session
                .get(librqbit::api::TorrentIdOrHash::Id(st.torrent_id))
                .map(|h| (st.torrent_id, h))
        });

        let (torrent_id, handle) = if let Some((id, h)) = existing_handle {
            if h.is_paused() {
                self.session
                    .unpause(&h)
                    .await
                    .map_err(|e| BackendError::Torrent(e.to_string()))?;
            }
            (id, h)
        } else {
            let (add, initial_peers) = match ctx.job.source() {
                Source::Magnet(uri) => {
                    let peers = extract_peers_from_magnet(uri);
                    let add = AddTorrent::from_url(uri.as_str());
                    (add, if peers.is_empty() { None } else { Some(peers) })
                }
                Source::TorrentFile(path) => {
                    let bytes = tokio::fs::read(path).await.map_err(BackendError::Io)?;
                    (AddTorrent::from_bytes(bytes), None)
                }
                Source::Url(_) => return Err(BackendError::Unsupported),
            };

            let mut add_opts = AddTorrentOptions {
                paused: false,
                output_folder: Some(ctx.job.target_dir().to_string_lossy().to_string()),
                initial_peers,
                overwrite: true,
                ..Default::default()
            };

            if let Some(selected) = ctx.job.options().selected_files.as_ref() {
                add_opts.only_files = Some(selected.iter().map(|&i| i as usize).collect());
            }

            let resp = self
                .session
                .add_torrent(add, Some(add_opts))
                .await
                .map_err(|e| BackendError::Torrent(e.to_string()))?;

            let (id, h) = match resp {
                AddTorrentResponse::Added(id, h) => (id, h),
                AddTorrentResponse::AlreadyManaged(id, h) => (id, h),
                AddTorrentResponse::ListOnly(_) => {
                    return Err(BackendError::Torrent("unexpected list_only".into()));
                }
            };

            let state_json = serde_json::to_string(&TorrentResumeState { torrent_id: id })
                .map_err(|e| BackendError::Torrent(e.to_string()))?;
            ctx.store.set_backend_state(ctx.job.id(), &state_json)?;
            (id, h)
        };

        handle
            .wait_until_initialized()
            .await
            .map_err(|e| BackendError::Torrent(e.to_string()))?;

        let target_seed_ratio = ctx
            .job
            .options()
            .seed_ratio
            .unwrap_or(self.default_seed_ratio);
        let mut is_seeding = false;

        loop {
            match ctx.control.command() {
                odm_core::backend::ControlCommand::Pause => {
                    let _ = self.session.pause(&handle).await;
                    return Ok(RunOutcome::Paused);
                }
                odm_core::backend::ControlCommand::Cancel => {
                    let _ = self
                        .session
                        .delete(librqbit::api::TorrentIdOrHash::Id(torrent_id), false)
                        .await;
                    return Ok(RunOutcome::Cancelled);
                }
                odm_core::backend::ControlCommand::Run => {}
            }

            let stats = handle.stats();
            ctx.progress
                .report(stats.progress_bytes, Some(stats.total_bytes));

            let download_finished = stats.finished
                || (stats.total_bytes > 0 && stats.progress_bytes >= stats.total_bytes);

            if download_finished {
                if target_seed_ratio > 0.0 {
                    if !is_seeding {
                        is_seeding = true;
                        if let Ok(mut current_job) = ctx.store.get_job(ctx.job.id())
                            && current_job.transition_to(JobState::Seeding).is_ok()
                        {
                            let _ = ctx.store.update_job(&current_job);
                        }
                    }
                    let downloaded = stats.progress_bytes.max(1) as f64;
                    let current_ratio = stats.uploaded_bytes as f64 / downloaded;
                    if current_ratio >= target_seed_ratio {
                        let _ = self.session.pause(&handle).await;
                        return Ok(RunOutcome::Completed);
                    }
                } else {
                    let _ = self.session.pause(&handle).await;
                    return Ok(RunOutcome::Completed);
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
