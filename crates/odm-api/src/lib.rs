//! Request, response and event types shared by the odm daemon and its clients.
#![forbid(unsafe_code)]

pub mod category;
pub mod checksum;
pub mod download;
pub mod error;
pub mod event;
pub mod grabber;
pub mod health;
pub mod plugin;
pub mod probe;
pub mod queue;
pub mod settings;

pub use category::{CategoryDto, ListCategoriesResponse, SetCategoryRequest};
pub use checksum::{ChecksumAlgorithm, ChecksumDto, ParseChecksumError};

pub use download::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, DownloadItemDto, JobOptionsDto,
    ListDownloadsResponse, SegmentDto,
};
pub use error::{ErrorDetail, ErrorResponse};
pub use event::{Event, WsAuthRequest, WsAuthResponse};
pub use grabber::{GrabbedFileDto, GrabberRequest, GrabberResponse};
pub use health::HealthResponse;
pub use plugin::{PluginDto, PluginListResponse};
pub use probe::{ProbeRequest, ProbeResponse, QualityDto, TorrentFileDto};
pub use queue::{CreateQueueRequest, ListQueuesResponse, QueueDto, UpdateQueueRequest};
pub use settings::{SettingsDto, UpdateSettingsRequest};
