//! Message types exchanged between the UI thread and the background bridge.
use odm_api::{
    AddDownloadRequest, DownloadDetailDto, DownloadItemDto, Event, ProbeResponse,
    UpdateSettingsRequest,
};

/// Actions dispatched from the UI thread to the background daemon bridge.
#[derive(Debug, Clone)]
pub enum UserAction {
    AddDownload(AddDownloadRequest),
    Probe(String),
    Pause(i64),
    Resume(i64),
    Cancel(i64),
    Remove { id: i64, delete_file: bool },
    UpdateSettings(UpdateSettingsRequest),
    CreateQueue { name: String, concurrency: u32 },
    Refresh,
}

/// Events sent from the background daemon bridge to the UI thread.
#[derive(Debug, Clone)]
pub enum GuiEvent {
    DaemonStatus(bool),
    DownloadsList(Vec<DownloadItemDto>),
    DownloadDetail(DownloadDetailDto),
    ProbeResult(Result<ProbeResponse, String>),
    EngineEvent(Event),
    ErrorMessage(String),
    UrlCaptured(String),
}
