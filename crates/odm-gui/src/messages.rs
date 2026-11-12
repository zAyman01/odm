//! Message types exchanged between the UI thread and the background bridge.
use odm_api::{
    AddDownloadRequest, CategoryDto, DownloadDetailDto, DownloadItemDto, Event, ProbeResponse,
    QueueDto, UpdateSettingsRequest,
};

/// Actions dispatched from the UI thread to the background daemon bridge.
#[derive(Debug, Clone)]
pub enum UserAction {
    AddDownload(Box<AddDownloadRequest>),
    Probe(String),
    SelectQuality(String),
    Pause(i64),
    Resume(i64),
    Cancel(i64),
    Remove {
        id: i64,
        delete_file: bool,
    },
    UpdateSettings(UpdateSettingsRequest),
    CreateQueue {
        name: String,
        concurrency: u32,
    },
    SetCategory {
        name: String,
        directory: Option<String>,
        extensions: Option<Vec<String>>,
    },
    UpdateQueueSchedule {
        name: String,
        start_time: String,
        stop_time: Option<String>,
        days: Option<Vec<String>>,
    },
    Refresh,
}

/// Events sent from the background daemon bridge to the UI thread.
#[derive(Debug, Clone)]
pub enum GuiEvent {
    DaemonStatus(bool),
    DownloadsList(Vec<DownloadItemDto>),
    CategoriesList(Vec<CategoryDto>),
    QueuesList(Vec<QueueDto>),
    DownloadDetail(Box<DownloadDetailDto>),
    ProbeResult(Result<ProbeResponse, String>),
    EngineEvent(Event),
    ErrorMessage(String),
    UrlCaptured(String),
}
