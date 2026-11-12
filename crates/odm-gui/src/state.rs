//! View-state models and pure reducer functions for the odm GUI.
use odm_api::{DownloadDetailDto, DownloadItemDto, Event, ProbeResponse, TorrentFileDto};

use crate::messages::GuiEvent;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Active,
    Completed,
    Torrents,
    Queue(String),
    Category(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone)]
pub struct AddDialogState {
    pub open: bool,
    pub url: String,
    pub filename: String,
    pub target_dir: String,
    pub segments: u32,
    pub speed_limit: u64,
    pub queue: String,
    pub probed: Option<ProbeResponse>,
    pub probing: bool,
    pub error: Option<String>,
    pub checksum_algo: String,
    pub checksum_hash: String,
    pub mirrors: String,
    pub category: Option<String>,
    pub batch_preview: Vec<String>,
    pub torrent_files: Vec<TorrentFileDto>,
    pub selected_file_indices: std::collections::HashSet<u32>,
    pub seed_ratio: Option<f64>,
}

impl Default for AddDialogState {
    fn default() -> Self {
        let default_dir = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "./downloads".to_string());

        Self {
            open: false,
            url: String::new(),
            filename: String::new(),
            target_dir: default_dir,
            segments: 8,
            speed_limit: 0,
            queue: "default".to_string(),
            probed: None,
            probing: false,
            error: None,
            checksum_algo: "sha256".to_string(),
            checksum_hash: String::new(),
            mirrors: String::new(),
            category: None,
            batch_preview: Vec::new(),
            torrent_files: Vec::new(),
            selected_file_indices: std::collections::HashSet::new(),
            seed_ratio: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsDialogState {
    pub open: bool,
    pub download_dir: String,
    pub default_segments: u32,
    pub global_speed_limit: u64,
    pub clipboard_monitoring: bool,
    pub theme: ThemeMode,
    pub active_tab: SettingsTab,
    pub selected_queue: String,
    pub queue_start_time: String,
    pub queue_stop_time: String,
    pub queue_days: String,
    pub category_dirs: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Categories,
    Scheduler,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        let default_dir = directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| "./downloads".to_string());

        Self {
            open: false,
            download_dir: default_dir,
            default_segments: 8,
            global_speed_limit: 0,
            clipboard_monitoring: true,
            theme: ThemeMode::Dark,
            active_tab: SettingsTab::General,
            selected_queue: "default".to_string(),
            queue_start_time: String::new(),
            queue_stop_time: String::new(),
            queue_days: String::new(),
            category_dirs: std::collections::HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ViewState {
    pub downloads: Vec<DownloadItemDto>,
    pub selected_id: Option<i64>,
    pub selected_detail: Option<DownloadDetailDto>,
    pub filter: Filter,
    pub queues: Vec<String>,
    pub queue_details: Vec<odm_api::QueueDto>,
    pub categories: Vec<odm_api::CategoryDto>,
    pub daemon_connected: bool,
    pub add_dialog: AddDialogState,
    pub settings_dialog: SettingsDialogState,
    pub theme: ThemeMode,
    pub status_message: Option<String>,
    pub error_message: Option<String>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            downloads: Vec::new(),
            selected_id: None,
            selected_detail: None,
            filter: Filter::All,
            queues: vec!["default".to_string()],
            queue_details: Vec::new(),
            categories: Vec::new(),
            daemon_connected: false,
            add_dialog: AddDialogState::default(),
            settings_dialog: SettingsDialogState::default(),
            theme: ThemeMode::Dark,
            status_message: None,
            error_message: None,
        }
    }
}

impl ViewState {
    pub fn select_download(&mut self, id: Option<i64>) {
        self.selected_id = id;
        if id.is_none() {
            self.selected_detail = None;
        }
    }

    pub fn open_add_dialog(&mut self, prefill_url: Option<String>) {
        self.add_dialog.open = true;
        self.add_dialog.error = None;
        self.add_dialog.probed = None;
        self.add_dialog.probing = false;
        self.add_dialog.checksum_hash.clear();
        self.add_dialog.mirrors.clear();
        self.add_dialog.category = None;
        self.add_dialog.batch_preview.clear();
        self.add_dialog.torrent_files.clear();
        self.add_dialog.selected_file_indices.clear();
        self.add_dialog.seed_ratio = None;

        if let Some(url) = prefill_url {
            self.update_add_dialog_url(url);
        }
    }

    pub fn update_add_dialog_url(&mut self, url: String) {
        if url.contains('[') && url.contains(']') {
            self.add_dialog.batch_preview =
                odm_core::batch::expand_batch_url(&url).unwrap_or_default();
        } else {
            self.add_dialog.batch_preview.clear();
        }

        let fname = url
            .split('?')
            .next()
            .unwrap_or(&url)
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();

        self.add_dialog.url = url;
        if !fname.is_empty() && !fname.contains('[') {
            self.add_dialog.filename = fname;
        }
    }

    pub fn build_add_download_requests(&self) -> Vec<odm_api::AddDownloadRequest> {
        let urls = if !self.add_dialog.batch_preview.is_empty() {
            self.add_dialog.batch_preview.clone()
        } else if !self.add_dialog.url.is_empty() {
            vec![self.add_dialog.url.clone()]
        } else {
            Vec::new()
        };

        let checksum = if !self.add_dialog.checksum_hash.trim().is_empty() {
            let algo = match self.add_dialog.checksum_algo.to_ascii_lowercase().as_str() {
                "sha1" => odm_api::ChecksumAlgorithm::Sha1,
                "md5" => odm_api::ChecksumAlgorithm::Md5,
                _ => odm_api::ChecksumAlgorithm::Sha256,
            };
            Some(odm_api::ChecksumDto {
                algorithm: algo,
                digest: self.add_dialog.checksum_hash.trim().to_string(),
            })
        } else {
            None
        };

        let mirrors = if !self.add_dialog.mirrors.trim().is_empty() {
            let list: Vec<String> = self
                .add_dialog
                .mirrors
                .lines()
                .flat_map(|line| line.split(','))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if list.is_empty() { None } else { Some(list) }
        } else {
            None
        };

        let selected_files = if !self.add_dialog.torrent_files.is_empty() {
            let mut indices: Vec<u32> = self
                .add_dialog
                .selected_file_indices
                .iter()
                .copied()
                .collect();
            indices.sort_unstable();
            Some(indices)
        } else {
            None
        };

        let options = Some(odm_api::JobOptionsDto {
            segments: Some(self.add_dialog.segments),
            speed_limit: if self.add_dialog.speed_limit == 0 {
                None
            } else {
                Some(self.add_dialog.speed_limit)
            },
            headers: None,
            user_agent: None,
            referer: None,
            cookies: None,
            checksum,
            mirrors,
            selected_files,
            seed_ratio: self.add_dialog.seed_ratio,
        });

        let single = urls.len() == 1;
        urls.into_iter()
            .map(|url| odm_api::AddDownloadRequest {
                url,
                filename: if single && !self.add_dialog.filename.is_empty() {
                    Some(self.add_dialog.filename.clone())
                } else {
                    None
                },
                target_dir: if self.add_dialog.target_dir.is_empty() {
                    None
                } else {
                    Some(self.add_dialog.target_dir.clone())
                },
                queue: Some(self.add_dialog.queue.clone()),
                category: self.add_dialog.category.clone(),
                options: options.clone(),
            })
            .collect()
    }

    pub fn close_add_dialog(&mut self) {
        self.add_dialog.open = false;
        self.add_dialog.url.clear();
        self.add_dialog.filename.clear();
        self.add_dialog.probed = None;
        self.add_dialog.error = None;
        self.add_dialog.checksum_hash.clear();
        self.add_dialog.mirrors.clear();
        self.add_dialog.category = None;
        self.add_dialog.batch_preview.clear();
        self.add_dialog.torrent_files.clear();
        self.add_dialog.selected_file_indices.clear();
        self.add_dialog.seed_ratio = None;
    }

    pub fn open_settings_dialog(&mut self) {
        self.settings_dialog.open = true;
    }

    pub fn close_settings_dialog(&mut self) {
        self.settings_dialog.open = false;
    }

    pub fn filtered_downloads(&self) -> Vec<&DownloadItemDto> {
        self.downloads
            .iter()
            .filter(|item| match self.filter {
                Filter::All => true,
                Filter::Active => {
                    item.state == "downloading"
                        || item.state == "connecting"
                        || item.state == "pending"
                        || item.state == "seeding"
                }
                Filter::Completed => item.state == "completed",
                Filter::Torrents => item.backend.as_deref() == Some("torrent"),
                Filter::Queue(ref q) => &item.queue == q,
                Filter::Category(ref cat) => item.category.as_deref() == Some(cat.as_str()),
            })
            .collect()
    }

    pub fn apply_event(&mut self, event: GuiEvent) {
        match event {
            GuiEvent::DaemonStatus(connected) => {
                self.daemon_connected = connected;
            }
            GuiEvent::DownloadsList(list) => {
                self.downloads = list;
                if let Some(selected) = self.selected_id
                    && !self.downloads.iter().any(|d| d.id == selected)
                {
                    self.selected_id = None;
                    self.selected_detail = None;
                }
            }
            GuiEvent::DownloadDetail(detail) => {
                if self.selected_id == Some(detail.item.id) {
                    self.selected_detail = Some(*detail);
                }
            }
            GuiEvent::ProbeResult(res) => {
                self.add_dialog.probing = false;
                match res {
                    Ok(probe) => {
                        if !probe.filename.is_empty() {
                            self.add_dialog.filename = probe.filename.clone();
                        }
                        if let Some(ref files) = probe.torrent_files {
                            self.add_dialog.torrent_files = files.clone();
                            self.add_dialog.selected_file_indices =
                                files.iter().map(|f| f.index).collect();
                        }
                        self.add_dialog.probed = Some(probe);
                    }
                    Err(err) => {
                        self.add_dialog.error = Some(err);
                    }
                }
            }
            GuiEvent::EngineEvent(evt) => match evt {
                Event::JobAdded(item) => {
                    if !self.downloads.iter().any(|d| d.id == item.id) {
                        self.downloads.push(item);
                    }
                }
                Event::JobStateChanged { id, state } => {
                    if let Some(item) = self.downloads.iter_mut().find(|d| d.id == id) {
                        item.state = state.clone();
                    }
                    if let Some(ref mut detail) = self.selected_detail
                        && detail.item.id == id
                    {
                        detail.item.state = state;
                    }
                }
                Event::JobProgress {
                    id,
                    downloaded_bytes,
                    total_bytes,
                    speed_bps,
                } => {
                    if let Some(item) = self.downloads.iter_mut().find(|d| d.id == id) {
                        item.downloaded_bytes = downloaded_bytes;
                        item.total_bytes = total_bytes;
                        item.speed_bps = speed_bps;
                        if let Some(total) = total_bytes
                            && total > 0
                        {
                            item.progress_pct =
                                ((downloaded_bytes as f64 / total as f64) * 100.0) as f32;
                        }
                    }
                    if let Some(ref mut detail) = self.selected_detail
                        && detail.item.id == id
                    {
                        detail.item.downloaded_bytes = downloaded_bytes;
                        detail.item.total_bytes = total_bytes;
                        detail.item.speed_bps = speed_bps;
                        if let Some(total) = total_bytes
                            && total > 0
                        {
                            detail.item.progress_pct =
                                ((downloaded_bytes as f64 / total as f64) * 100.0) as f32;
                        }
                    }
                }
                Event::JobRemoved(id) => {
                    self.downloads.retain(|d| d.id != id);
                    if self.selected_id == Some(id) {
                        self.selected_id = None;
                        self.selected_detail = None;
                    }
                }
                Event::QueueChanged(q) => {
                    if !self.queues.contains(&q.name) {
                        self.queues.push(q.name);
                    }
                }
            },
            GuiEvent::ErrorMessage(msg) => {
                self.error_message = Some(msg);
            }
            GuiEvent::CategoriesList(list) => {
                self.categories = list;
            }
            GuiEvent::QueuesList(list) => {
                self.queue_details = list.clone();
                self.queues = list.into_iter().map(|q| q.name).collect();
                if !self.queues.contains(&"default".to_string()) {
                    self.queues.insert(0, "default".to_string());
                }
            }
            GuiEvent::UrlCaptured(url) => {
                self.open_add_dialog(Some(url));
            }
        }
    }
}
