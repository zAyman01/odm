//! View-state models and pure reducer functions for the odm GUI.
use odm_api::{
    DownloadDetailDto, DownloadItemDto, Event, GrabbedFileDto, GrabberRequest, PluginDto,
    ProbeResponse, QualityDto, TorrentFileDto, UpdateSettingsRequest,
};

use std::time::{Duration, Instant};

use crate::messages::{GuiEvent, UserAction};

/// How often the detail panel asks the daemon for fresh segments.
const DETAIL_REFRESH: Duration = Duration::from_secs(2);

/// States that count as "active" in the sidebar and its filter.
pub fn is_active_state(state: &str) -> bool {
    matches!(state, "downloading" | "probing" | "verifying" | "seeding")
}

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
pub struct GrabberViewState {
    pub is_open: bool,
    pub url: String,
    pub depth: u32,
    pub extensions: String,
    pub pattern: String,
    pub respect_robots: bool,
    pub max_pages: usize,
    pub is_crawling: bool,
    pub error: Option<String>,
    pub files: Vec<GrabbedFileDto>,
    pub pages_crawled: usize,
    pub selected_indices: std::collections::HashSet<usize>,
}

impl Default for GrabberViewState {
    fn default() -> Self {
        Self {
            is_open: false,
            url: String::new(),
            depth: 1,
            extensions: String::new(),
            pattern: String::new(),
            respect_robots: true,
            max_pages: 50,
            is_crawling: false,
            error: None,
            files: Vec::new(),
            pages_crawled: 0,
            selected_indices: std::collections::HashSet::new(),
        }
    }
}

impl GrabberViewState {
    pub fn open(&mut self) {
        self.is_open = true;
        self.error = None;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn toggle_file(&mut self, idx: usize) {
        if self.selected_indices.contains(&idx) {
            self.selected_indices.remove(&idx);
        } else {
            self.selected_indices.insert(idx);
        }
    }

    pub fn select_all(&mut self) {
        self.selected_indices = (0..self.files.len()).collect();
    }

    pub fn deselect_all(&mut self) {
        self.selected_indices.clear();
    }
}

#[derive(Debug, Clone, Default)]
pub struct PluginsViewState {
    pub is_open: bool,
    pub plugins: Vec<PluginDto>,
    pub is_loading: bool,
    pub error: Option<String>,
}

impl PluginsViewState {
    pub fn open(&mut self) {
        self.is_open = true;
        self.error = None;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn has_security_notice(&self) -> bool {
        true
    }

    pub fn security_notice_text(&self) -> &'static str {
        "Security Notice: Plugins are external programs running with full user permissions without sandboxing. Only install plugins from trusted sources."
    }
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
    pub qualities: Vec<QualityDto>,
    pub selected_quality: Option<String>,
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
            qualities: Vec::new(),
            selected_quality: None,
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
    pub ffmpeg_path: String,
    pub ytdlp_path: String,
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
            ffmpeg_path: String::new(),
            ytdlp_path: String::new(),
        }
    }
}

impl SettingsDialogState {
    pub fn load_settings(&mut self, settings: &odm_api::SettingsDto) {
        if let Some(ref dir) = settings.download_dir {
            self.download_dir = dir.clone();
        }
        self.default_segments = settings.default_segments;
        self.global_speed_limit = settings.global_speed_limit;
        self.ffmpeg_path = settings.ffmpeg_path.clone();
        self.ytdlp_path = settings.ytdlp_path.clone();
    }

    pub fn build_update_settings_request(&self) -> UpdateSettingsRequest {
        UpdateSettingsRequest {
            download_dir: if self.download_dir.is_empty() {
                None
            } else {
                Some(self.download_dir.clone())
            },
            default_segments: Some(self.default_segments),
            global_speed_limit: if self.global_speed_limit == 0 {
                None
            } else {
                Some(self.global_speed_limit)
            },
            default_retry_count: None,
            seed_ratio: None,
            torrent_listen_port: None,
            ffmpeg_path: if self.ffmpeg_path.is_empty() {
                None
            } else {
                Some(self.ffmpeg_path.clone())
            },
            ytdlp_path: if self.ytdlp_path.is_empty() {
                None
            } else {
                Some(self.ytdlp_path.clone())
            },
            allowed_hosts: None,
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
    pub grabber: GrabberViewState,
    pub plugins: PluginsViewState,
    pub theme: ThemeMode,
    pub status_message: Option<String>,
    pub error_message: Option<String>,
    /// Name typed into the sidebar's "new queue" form.
    pub new_queue_name: String,
    pub new_queue_concurrency: u32,
    /// The job whose detail was last requested, and when.
    pub detail_requested: Option<(i64, Instant)>,
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
            grabber: GrabberViewState::default(),
            plugins: PluginsViewState::default(),
            theme: ThemeMode::Dark,
            status_message: None,
            error_message: None,
            new_queue_name: String::new(),
            new_queue_concurrency: 3,
            detail_requested: None,
        }
    }
}

impl ViewState {
    pub fn select_download(&mut self, id: Option<i64>) {
        self.selected_id = id;
        if id.is_none() {
            self.selected_detail = None;
            self.detail_requested = None;
        }
    }

    /// The job whose detail the daemon should be asked for now: the selected one,
    /// when it is new or its detail is older than `DETAIL_REFRESH`.
    pub fn next_detail_request(&mut self, now: Instant) -> Option<i64> {
        let id = self.selected_id?;
        let due = match self.detail_requested {
            Some((requested, at)) if requested == id => {
                now.saturating_duration_since(at) >= DETAIL_REFRESH
            }
            _ => true,
        };
        if !due {
            return None;
        }
        self.detail_requested = Some((id, now));
        Some(id)
    }

    /// Turns the "new queue" form into an action and clears the name.
    pub fn take_new_queue_action(&mut self) -> Option<UserAction> {
        let name = self.new_queue_name.trim().to_string();
        if name.is_empty() {
            return None;
        }
        self.new_queue_name.clear();
        Some(UserAction::CreateQueue {
            name,
            concurrency: self.new_queue_concurrency.max(1),
        })
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
        self.add_dialog.qualities.clear();
        self.add_dialog.selected_quality = None;

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
            quality: self.add_dialog.selected_quality.clone(),
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
                backend: None,
            })
            .collect()
    }

    pub fn build_add_request(&self) -> Option<odm_api::AddDownloadRequest> {
        self.build_add_download_requests().into_iter().next()
    }

    pub fn build_update_settings_request(&self) -> UpdateSettingsRequest {
        self.settings_dialog.build_update_settings_request()
    }

    pub fn handle_action(&mut self, action: &crate::messages::UserAction) {
        if let crate::messages::UserAction::SelectQuality(id) = action {
            self.add_dialog.selected_quality = Some(id.clone());
        }
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
        self.add_dialog.qualities.clear();
        self.add_dialog.selected_quality = None;
    }

    pub fn open_settings_dialog(&mut self) {
        self.settings_dialog.open = true;
    }

    pub fn close_settings_dialog(&mut self) {
        self.settings_dialog.open = false;
    }

    pub fn open_grabber_dialog(&mut self) {
        self.grabber.open();
    }

    pub fn close_grabber_dialog(&mut self) {
        self.grabber.close();
    }

    pub fn open_plugins_dialog(&mut self) {
        self.plugins.open();
    }

    pub fn close_plugins_dialog(&mut self) {
        self.plugins.close();
    }

    pub fn build_grab_request(&self) -> Option<GrabberRequest> {
        if self.grabber.url.trim().is_empty() {
            return None;
        }
        let extensions = if self.grabber.extensions.trim().is_empty() {
            None
        } else {
            let list: Vec<String> = self
                .grabber
                .extensions
                .split(',')
                .map(|s| s.trim().trim_start_matches('.').to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if list.is_empty() { None } else { Some(list) }
        };
        let pattern = if self.grabber.pattern.trim().is_empty() {
            None
        } else {
            Some(self.grabber.pattern.trim().to_string())
        };

        Some(GrabberRequest {
            url: self.grabber.url.trim().to_string(),
            depth: Some(self.grabber.depth),
            extensions,
            pattern,
            respect_robots: Some(self.grabber.respect_robots),
            max_pages: Some(self.grabber.max_pages),
        })
    }

    pub fn build_grabbed_add_requests(&self) -> Vec<odm_api::AddDownloadRequest> {
        self.grabber
            .files
            .iter()
            .enumerate()
            .filter(|(idx, _)| self.grabber.selected_indices.contains(idx))
            .map(|(_, file)| odm_api::AddDownloadRequest {
                url: file.url.clone(),
                filename: Some(file.filename.clone()),
                target_dir: None,
                queue: None,
                category: None,
                options: None,
                backend: None,
            })
            .collect()
    }

    pub fn filtered_downloads(&self) -> Vec<&DownloadItemDto> {
        self.downloads
            .iter()
            .filter(|item| match self.filter {
                Filter::All => true,
                Filter::Active => is_active_state(&item.state),
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
            GuiEvent::SettingsLoaded(settings) => {
                self.settings_dialog.load_settings(&settings);
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
                        if let Some(ref qualities) = probe.qualities {
                            self.add_dialog.qualities = qualities.clone();
                            self.add_dialog.selected_quality =
                                qualities.first().map(|q| q.id.clone());
                        }
                        self.add_dialog.probed = Some(probe);
                    }
                    Err(err) => {
                        self.add_dialog.error = Some(err);
                    }
                }
            }
            GuiEvent::GrabberResult(res) => {
                self.grabber.is_crawling = false;
                match res {
                    Ok(resp) => {
                        self.grabber.pages_crawled = resp.pages_crawled;
                        self.grabber.selected_indices = (0..resp.files.len()).collect();
                        self.grabber.files = resp.files;
                        self.grabber.error = None;
                    }
                    Err(err) => {
                        self.grabber.error = Some(err);
                    }
                }
            }
            GuiEvent::PluginsList(res) => {
                self.plugins.is_loading = false;
                match res {
                    Ok(resp) => {
                        self.plugins.plugins = resp.plugins;
                        self.plugins.error = None;
                    }
                    Err(err) => {
                        self.plugins.error = Some(err);
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

pub fn backend_display(backend: Option<&str>) -> &str {
    match backend {
        Some("hls") => "HLS",
        Some("dash") => "DASH",
        Some("yt-dlp") | Some("ytdlp") => "yt-dlp",
        Some("torrent") => "Torrent",
        Some("http") => "HTTP",
        Some(other) => other,
        None => "HTTP",
    }
}
