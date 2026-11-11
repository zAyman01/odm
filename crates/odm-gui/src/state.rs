//! View-state models and pure reducer functions for the odm GUI.
use odm_api::{DownloadDetailDto, DownloadItemDto, Event, ProbeResponse};

use crate::messages::GuiEvent;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Active,
    Completed,
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

        if let Some(url) = prefill_url {
            let fname = url
                .split('?')
                .next()
                .unwrap_or(&url)
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();

            self.add_dialog.url = url;
            if !fname.is_empty() {
                self.add_dialog.filename = fname;
            }
        }
    }

    pub fn close_add_dialog(&mut self) {
        self.add_dialog.open = false;
        self.add_dialog.url.clear();
        self.add_dialog.filename.clear();
        self.add_dialog.probed = None;
        self.add_dialog.error = None;
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
                }
                Filter::Completed => item.state == "completed",
                Filter::Queue(ref q) => &item.queue == q,
                Filter::Category(ref cat) => {
                    let ext = item.filename.rsplit('.').next().unwrap_or("");
                    match cat.as_str() {
                        "Documents" => matches!(ext, "pdf" | "doc" | "docx" | "txt"),
                        "Archives" => matches!(ext, "zip" | "tar" | "gz" | "7z" | "rar"),
                        "Media" => matches!(ext, "mp4" | "mkv" | "mp3" | "flac" | "wav"),
                        _ => true,
                    }
                }
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
                    self.selected_detail = Some(detail);
                }
            }
            GuiEvent::ProbeResult(res) => {
                self.add_dialog.probing = false;
                match res {
                    Ok(probe) => {
                        if !probe.filename.is_empty() {
                            self.add_dialog.filename = probe.filename.clone();
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
            GuiEvent::UrlCaptured(url) => {
                self.open_add_dialog(Some(url));
            }
        }
    }
}
