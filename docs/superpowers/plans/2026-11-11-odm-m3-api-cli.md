# odm M3: Daemon API and CLI — Implementation Plan

> **For agentic workers:** Do the tasks in order, one at a time, every step in order. Tick each checkbox as you finish it. If a command's output does not match the "Expected" line, stop and report; do not work around it.

**Goal:** Provide a local REST and WebSocket API service (`odm-daemon`) over the download engine with bearer authentication, DNS-rebinding protection and CORS, plus a full-featured command-line client (`odm-cli` binary `odm`) supporting human-readable and JSON output modes.

**Milestone Success Criterion:** `odm add <url>` downloads a real file from a local test HTTP server via the daemon API and CLI.

**Architecture:**
- `odm-api`: Data Transfer Objects (DTOs) for all REST requests, responses, errors, and WebSocket events.
- `odm-core`: Migration 3 adds the `settings` table, job/queue deletion, and an event bus broadcast channel emitting live progress and state changes.
- `odm-daemon`: Headless Axum server binding `127.0.0.1:6820`. Generates an owner-only auth token in the config directory. Implements bearer token validation on all routes except `/api/v1/health`, `Host` header validation against loopback to prevent DNS rebinding, CORS validation, REST endpoints under `/api/v1`, WebSocket `/api/v1/events` streaming, and concurrency-managed background execution of added/resumed jobs.
- `odm-cli` (binary `odm`): Built with `clap`. Implements `add`, `list`, `info`, `pause`, `resume`, `cancel`, `retry`, `remove`, `queue`, and `daemon status`. Reads auth token automatically from config directory or CLI/env flag. Supports `--json` flag on all subcommands.

**Spec:** `docs/superpowers/specs/2026-11-11-odm-design.md` (sections 2, 5, 6, 7, 8 and milestone M3 in section 12).

---

## Global Constraints

- Run every command from the repository root `D:\Development\Projects\IDM`.
- When running in PowerShell, run commands sequentially or use `;` (not `&&`).
- Workspace uses Rust `1.99.1`, edition `2024`, license `MIT OR Apache-2.0`.
- `#![forbid(unsafe_code)]` stays in every crate root (`lib.rs` and `main.rs`).
- No `unwrap()` or `expect()` outside test code. Production code returns typed `Result`s.
- Tests never use the real internet. All HTTP tests use local servers on `127.0.0.1`.
- One commit per task, with the exact commit message specified by the task.
- Every new test file under `crates/*/tests/` starts with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- After adding dependencies, run `cargo deny check` to ensure licenses and bans comply.
- Do not push, tag, create repositories, or publish crates.

---

## File Structure

```
crates/odm-api/Cargo.toml
crates/odm-api/src/lib.rs
crates/odm-api/src/download.rs
crates/odm-api/src/error.rs
crates/odm-api/src/event.rs
crates/odm-api/src/health.rs
crates/odm-api/src/probe.rs
crates/odm-api/src/queue.rs
crates/odm-api/src/settings.rs
crates/odm-api/tests/models.rs

crates/odm-core/src/event.rs
crates/odm-core/src/store.rs
crates/odm-core/src/engine.rs
crates/odm-core/src/lib.rs
crates/odm-core/tests/store_v3.rs
crates/odm-core/tests/engine_events.rs

crates/odm-daemon/Cargo.toml
crates/odm-daemon/src/lib.rs
crates/odm-daemon/src/config.rs
crates/odm-daemon/src/auth.rs
crates/odm-daemon/src/state.rs
crates/odm-daemon/src/routes.rs
crates/odm-daemon/src/ws.rs
crates/odm-daemon/src/server.rs
crates/odm-daemon/src/main.rs
crates/odm-daemon/tests/auth_and_security.rs
crates/odm-daemon/tests/api_routes.rs
crates/odm-daemon/tests/websocket_events.rs

crates/odm-cli/Cargo.toml
crates/odm-cli/src/main.rs
crates/odm-cli/src/client.rs
crates/odm-cli/src/commands.rs
crates/odm-cli/src/format.rs
crates/odm-cli/tests/cli_commands.rs
crates/odm-cli/tests/e2e_download.rs

CHANGELOG.md
```

---

### Task 1: `odm-api` shared contract and event models

**Files:**
- Modify: `crates/odm-api/Cargo.toml`
- Create: `crates/odm-api/src/error.rs`
- Create: `crates/odm-api/src/health.rs`
- Create: `crates/odm-api/src/probe.rs`
- Create: `crates/odm-api/src/download.rs`
- Create: `crates/odm-api/src/queue.rs`
- Create: `crates/odm-api/src/settings.rs`
- Create: `crates/odm-api/src/event.rs`
- Modify: `crates/odm-api/src/lib.rs`
- Create: `crates/odm-api/tests/models.rs`

**Interfaces:**
- Consumes: `serde`, `serde_json`.
- Produces: `ErrorResponse`, `ErrorDetail`, `HealthResponse`, `ProbeRequest`, `ProbeResponse`, `JobOptionsDto`, `AddDownloadRequest`, `AddDownloadResponse`, `DownloadItemDto`, `SegmentDto`, `DownloadDetailDto`, `ListDownloadsResponse`, `QueueDto`, `CreateQueueRequest`, `UpdateQueueRequest`, `ListQueuesResponse`, `SettingsDto`, `UpdateSettingsRequest`, `WsAuthRequest`, `WsAuthResponse`, `Event`.

- [x] **Step 1: Write serialization & deserialization tests first**

`crates/odm-api/tests/models.rs`:

```rust
//! API model serialization and contract compliance tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadItemDto, ErrorResponse, Event, HealthResponse,
    JobOptionsDto, ListDownloadsResponse, ListQueuesResponse, ProbeResponse, QueueDto, SettingsDto,
    UpdateSettingsRequest, WsAuthRequest,
};

#[test]
fn error_response_matches_spec_shape() {
    let err = ErrorResponse::new("invalid_token", "Bearer token is invalid");
    let json = serde_json::to_string(&err).unwrap();
    assert_eq!(
        json,
        r#"{"error":{"code":"invalid_token","message":"Bearer token is invalid"}}"#
    );

    let parsed: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.error.code, "invalid_token");
    assert_eq!(parsed.error.message, "Bearer token is invalid");
}

#[test]
fn health_response_roundtrips() {
    let health = HealthResponse {
        version: "0.1.0".to_string(),
        status: "ok".to_string(),
    };
    let json = serde_json::to_string(&health).unwrap();
    assert_eq!(json, r#"{"version":"0.1.0","status":"ok"}"#);
    let parsed: HealthResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, "0.1.0");
    assert_eq!(parsed.status, "ok");
}

#[test]
fn add_download_request_and_response() {
    let req = AddDownloadRequest {
        url: "https://example.com/file.zip".to_string(),
        target_dir: Some("/downloads".to_string()),
        filename: Some("custom.zip".to_string()),
        queue: Some("bulk".to_string()),
        options: Some(JobOptionsDto {
            segments: Some(4),
            speed_limit: Some(1024),
            ..Default::default()
        }),
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: AddDownloadRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.url, "https://example.com/file.zip");
    assert_eq!(parsed.options.unwrap().segments, Some(4));

    let resp = AddDownloadResponse { id: 42 };
    let json_resp = serde_json::to_string(&resp).unwrap();
    assert_eq!(json_resp, r#"{"id":42}"#);
}

#[test]
fn download_item_dto_and_list() {
    let item = DownloadItemDto {
        id: 1,
        url: "https://example.com/file.bin".to_string(),
        filename: "file.bin".to_string(),
        state: "downloading".to_string(),
        total_bytes: Some(1024),
        downloaded_bytes: 512,
        progress_pct: 50.0,
        speed_bps: 10240,
        queue: "default".to_string(),
        target_dir: "/tmp".to_string(),
        error: None,
    };
    let list = ListDownloadsResponse {
        downloads: vec![item],
    };
    let json = serde_json::to_string(&list).unwrap();
    let parsed: ListDownloadsResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.downloads.len(), 1);
    assert_eq!(parsed.downloads[0].id, 1);
    assert_eq!(parsed.downloads[0].state, "downloading");
}

#[test]
fn event_variants_roundtrip_tagged_shape() {
    let event = Event::JobProgress {
        id: 10,
        downloaded_bytes: 2048,
        total_bytes: Some(4096),
        speed_bps: 512,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"job_progress""#));
    let parsed: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, event);

    let auth = WsAuthRequest {
        token: "secret123".to_string(),
    };
    let auth_json = serde_json::to_string(&auth).unwrap();
    assert_eq!(auth_json, r#"{"token":"secret123"}"#);
}
```

- [x] **Step 2: Add dependencies to `crates/odm-api/Cargo.toml`**

```toml
[dependencies]
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"

[dev-dependencies]
serde_json = "1.0.154"
```

- [x] **Step 3: Implement domain types in `crates/odm-api/src/`**

Create `crates/odm-api/src/error.rs`:
```rust
//! Error types and unified error response shape.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

impl ErrorResponse {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}
```

Create `crates/odm-api/src/health.rs`:
```rust
//! Health check models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub version: String,
    pub status: String,
}
```

Create `crates/odm-api/src/probe.rs`:
```rust
//! Source probing request and response.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProbeResponse {
    pub backend: String,
    pub filename: String,
    pub size: Option<u64>,
    pub ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}
```

Create `crates/odm-api/src/download.rs`:
```rust
//! Download job requests, responses and DTOs.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobOptionsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<(String, String)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddDownloadRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<JobOptionsDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddDownloadResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadItemDto {
    pub id: i64,
    pub url: String,
    pub filename: String,
    pub state: String,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub progress_pct: f32,
    pub speed_bps: u64,
    pub queue: String,
    pub target_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDto {
    pub index: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadDetailDto {
    pub item: DownloadItemDto,
    pub segments: Vec<SegmentDto>,
    pub options: JobOptionsDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListDownloadsResponse {
    pub downloads: Vec<DownloadItemDto>,
}
```

Create `crates/odm-api/src/queue.rs`:
```rust
//! Queue management models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueDto {
    pub id: i64,
    pub name: String,
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateQueueRequest {
    pub name: String,
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateQueueRequest {
    pub max_concurrent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListQueuesResponse {
    pub queues: Vec<QueueDto>,
}
```

Create `crates/odm-api/src/settings.rs`:
```rust
//! Application settings models.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    pub default_segments: u32,
    pub global_speed_limit: u64,
    pub default_retry_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_segments: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_speed_limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_retry_count: Option<u32>,
}
```

Create `crates/odm-api/src/event.rs`:
```rust
//! WebSocket events and authentication requests.
use serde::{Deserialize, Serialize};

use crate::download::DownloadItemDto;
use crate::queue::QueueDto;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WsAuthRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WsAuthResponse {
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    JobAdded(DownloadItemDto),
    JobStateChanged {
        id: i64,
        state: String,
    },
    JobProgress {
        id: i64,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed_bps: u64,
    },
    JobRemoved(i64),
    QueueChanged(QueueDto),
}
```

Update `crates/odm-api/src/lib.rs`:
```rust
//! Request, response and event types shared by the odm daemon and its clients.
#![forbid(unsafe_code)]

pub mod download;
pub mod error;
pub mod event;
pub mod health;
pub mod probe;
pub mod queue;
pub mod settings;

pub use download::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, DownloadItemDto, JobOptionsDto,
    ListDownloadsResponse, SegmentDto,
};
pub use error::{ErrorDetail, ErrorResponse};
pub use event::{Event, WsAuthRequest, WsAuthResponse};
pub use health::HealthResponse;
pub use probe::{ProbeRequest, ProbeResponse};
pub use queue::{CreateQueueRequest, ListQueuesResponse, QueueDto, UpdateQueueRequest};
pub use settings::{SettingsDto, UpdateSettingsRequest};
```

- [x] **Step 4: Run the models test and verify gate**

Run:
```bash
cargo test -p odm-api --test models
```
Expected: `test result: ok. 5 passed; 0 failed`.

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-api Cargo.lock
git commit -m "feat(api): define shared REST and WebSocket DTO models"
```

---

### Task 2: Store v3 (settings & delete), event bus and remove in `odm-core`

**Files:**
- Modify: `crates/odm-core/src/store.rs`
- Create: `crates/odm-core/src/event.rs`
- Modify: `crates/odm-core/src/engine.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Create: `crates/odm-core/tests/store_v3.rs`
- Create: `crates/odm-core/tests/engine_events.rs`

**Interfaces:**
- Consumes: `odm-core` store and engine.
- Produces:
  - `Store::get_setting`, `Store::set_setting`, `Store::delete_job`, `Store::delete_queue`.
  - `Event` (`CoreEvent`): `JobAdded`, `JobStateChanged`, `JobProgress`, `JobRemoved`, `QueueChanged`.
  - `Engine::subscribe(&self) -> broadcast::Receiver<Event>`.
  - `Engine::remove(&self, id: JobId, delete_file: bool) -> Result<(), EngineError>`.
  - `Engine::run` emits live progress (throttled to at most 4/sec) and state transitions.

- [x] **Step 1: Write store v3 and engine event tests first**

`crates/odm-core/tests/store_v3.rs`:
```rust
//! Store v3: settings key-value store and cascade job deletion.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use odm_core::{Job, JobOptions, Segment, Source, Store};
use tempfile::tempdir;

#[test]
fn migration_3_creates_settings_and_persists_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), 3);
    assert_eq!(store.get_setting("download_dir").unwrap(), None);

    store.set_setting("download_dir", "/downloads").unwrap();
    assert_eq!(
        store.get_setting("download_dir").unwrap().as_deref(),
        Some("/downloads")
    );

    // Overwrite setting
    store.set_setting("download_dir", "/new_downloads").unwrap();
    assert_eq!(
        store.get_setting("download_dir").unwrap().as_deref(),
        Some("/new_downloads")
    );
}

#[test]
fn delete_job_cascades_to_segments() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    let mut job = Job::new(
        Source::Url("https://example.com/file.bin".to_string()),
        dir.path().to_path_buf(),
        JobOptions::default(),
    );
    store.insert_job(&mut job).unwrap();
    let id = job.id();

    let segments = vec![Segment {
        index: 0,
        start: 0,
        end: 100,
        downloaded: 50,
    }];
    store.replace_segments(id, &segments).unwrap();
    assert_eq!(store.list_segments(id).unwrap().len(), 1);

    store.delete_job(id).unwrap();
    assert!(store.get_job(id).is_err());
    assert_eq!(store.list_segments(id).unwrap().len(), 0);
}

#[test]
fn cannot_delete_default_queue() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.sqlite");
    let store = Store::open(&path).unwrap();

    assert!(store.delete_queue("default").is_err());

    store.create_queue("temporary", 2).unwrap();
    assert!(store.delete_queue("temporary").is_ok());
    assert!(store.queue_by_name("temporary").is_err());
}
```

`crates/odm-core/tests/engine_events.rs`:
```rust
//! Engine event bus and live progress streaming.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use odm_core::{Engine, Event, JobOptions, JobState, Store};
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn test_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = hits.clone();

    let payload: Vec<u8> = vec![b'x'; 256 * 1024];

    let app = Router::new().route(
        "/events_file.bin",
        get(move |req: Request| {
            let hits = hits_clone.clone();
            let payload = payload.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, payload.len().to_string().leak()),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    payload,
                )
                    .into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}/events_file.bin"), hits)
}

#[tokio::test]
async fn engine_emits_added_state_and_removed_events() {
    let dir = tempdir().unwrap();
    let store = Store::open(&dir.path().join("store.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();

    let mut rx = engine.subscribe();
    let (url, _) = test_server().await;

    let job = engine
        .add(&url, dir.path().to_path_buf(), JobOptions::default())
        .unwrap();
    let id = job.id();

    // Event 1: JobAdded
    let event = rx.recv().await.unwrap();
    assert_eq!(event, Event::JobAdded(id));

    // Run to completion
    let finished_job = engine.run(id).await.unwrap();
    assert_eq!(finished_job.state(), JobState::Completed);

    // Collect subsequent events
    let mut states = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let Event::JobStateChanged { id: ev_id, state } = ev {
            if ev_id == id {
                states.push(state);
            }
        }
    }
    assert!(states.contains(&JobState::Downloading));
    assert!(states.contains(&JobState::Completed));

    // Remove job and file
    engine.remove(id, true).unwrap();
    let removed_event = rx.recv().await.unwrap();
    assert_eq!(removed_event, Event::JobRemoved(id));
}
```

- [x] **Step 2: Implement Store migration 3 and methods**

In `crates/odm-core/src/store.rs`:
Add migration 3:
```sql
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```
Update `CURRENT_VERSION = 3`.

Add methods to `Store`:
```rust
pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
    let connection = self.lock()?;
    let mut stmt = connection.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
    let connection = self.lock()?;
    connection.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn delete_job(&self, id: JobId) -> Result<(), StoreError> {
    let connection = self.lock()?;
    let affected = connection.execute("DELETE FROM jobs WHERE id = ?1", params![id.0])?;
    if affected == 0 {
        return Err(StoreError::JobNotFound(id));
    }
    Ok(())
}

pub fn delete_queue(&self, name: &str) -> Result<(), StoreError> {
    if name == "default" {
        return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
    }
    let connection = self.lock()?;
    let affected = connection.execute("DELETE FROM queues WHERE name = ?1", params![name])?;
    if affected == 0 {
        return Err(StoreError::QueueNotFound(name.to_string()));
    }
    Ok(())
}
```

- [x] **Step 3: Create `crates/odm-core/src/event.rs` and update `Engine`**

`crates/odm-core/src/event.rs`:
```rust
//! Broadcast event bus domain events.
use crate::job::{JobId, JobState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    JobAdded(JobId),
    JobStateChanged {
        id: JobId,
        state: JobState,
    },
    JobProgress {
        id: JobId,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        speed_bps: u64,
    },
    JobRemoved(JobId),
    QueueChanged(String),
}
```

Update `crates/odm-core/src/engine.rs`:
Add:
```rust
use tokio::sync::broadcast;
use crate::event::Event;
```
Add `events: broadcast::Sender<Event>` to `Engine`. Initialize in `Engine::new` with capacity `1024`.
Add:
```rust
pub fn subscribe(&self) -> broadcast::Receiver<Event> {
    self.events.subscribe()
}

pub fn remove(&self, id: JobId, delete_file: bool) -> Result<(), EngineError> {
    let _ = self.cancel(id);
    let job = self.store.get_job(id)?;
    self.store.delete_job(id)?;
    if delete_file {
        let file_path = job.target_dir().join(job.filename());
        let part_path = job.target_dir().join(format!("{}.part", job.filename()));
        let _ = std::fs::remove_file(file_path);
        let _ = std::fs::remove_file(part_path);
    }
    let _ = self.events.send(Event::JobRemoved(id));
    Ok(())
}
```
In `Engine::add`:
Emit `Event::JobAdded(job.id())`.

In `Engine::run`:
Emit `Event::JobStateChanged` upon transitions.
Concurrently drain `updates` from `ProgressSink`, calculate speed, throttle to at most 4 progress events/sec (250ms), and emit `Event::JobProgress`.

- [x] **Step 4: Run tests and verify gate**

Run:
```bash
cargo test -p odm-core --test store_v3
cargo test -p odm-core --test engine_events
```
Expected: all pass.

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-core Cargo.lock
git commit -m "feat(core): add settings store, job deletion and event broadcast"
```

---

### Task 3: Daemon authentication, config directory, host validation and CORS

**Files:**
- Modify: `crates/odm-daemon/Cargo.toml`
- Create: `crates/odm-daemon/src/lib.rs`
- Create: `crates/odm-daemon/src/config.rs`
- Create: `crates/odm-daemon/src/auth.rs`
- Create: `crates/odm-daemon/tests/auth_and_security.rs`

**Interfaces:**
- Consumes: `axum`, `tower-http`, `directories`, `fastrand`, `odm-api`.
- Produces:
  - `ConfigDirs`: resolution of config/data dirs, token generation with owner permissions (0600 on Unix).
  - `auth_middleware`: enforces `Authorization: Bearer <token>` on all routes except `/api/v1/health`.
  - `host_validation_middleware`: blocks DNS rebinding by requiring `Host` to match `127.0.0.1`, `localhost`, or loopback.
  - CORS middleware allowing loopback and browser extension origins.

- [x] **Step 1: Write auth and security tests first**

`crates/odm-daemon/tests/auth_and_security.rs`:
```rust
//! Daemon security middleware: bearer auth, host validation and CORS.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use odm_api::ErrorResponse;
use odm_daemon::auth::{create_security_layers, AuthState};
use odm_daemon::config::DaemonConfig;
use tempfile::tempdir;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_bypasses_auth() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let state = AuthState::new(config);
    let app = odm_daemon::server::build_router(state);

    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn unauthenticated_request_is_rejected_with_401() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let state = AuthState::new(config);
    let app = odm_daemon::server::build_router(state);

    let req = Request::builder()
        .uri("/api/v1/downloads")
        .header(header::HOST, "127.0.0.1:6820")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "unauthorized");
}

#[tokio::test]
async fn invalid_host_header_is_rejected_with_403() {
    let dir = tempdir().unwrap();
    let config = DaemonConfig::for_test(dir.path(), "test-secret-token");
    let state = AuthState::new(config);
    let app = odm_daemon::server::build_router(state);

    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "attacker.com")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let err: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err.error.code, "invalid_host");
}
```

- [x] **Step 2: Configure dependencies in `crates/odm-daemon/Cargo.toml`**

```toml
[dependencies]
axum = { version = "0.8.9", features = ["ws", "tokio", "http1", "json"] }
clap = { version = "4.6.7", features = ["derive"] }
directories = "6.0.0"
fastrand = "2.5.0"
odm-api = { path = "../odm-api" }
odm-core = { path = "../odm-core" }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
thiserror = "2.0.21"
tokio = { version = "1.54.1", features = ["macros", "rt-multi-thread", "net", "sync", "time", "signal"] }
tower = { version = "0.5.2", features = ["util"] }
tower-http = { version = "0.6.11", features = ["cors"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3.27.0"
tokio = { version = "1.54.1", features = ["test-util"] }
```

- [x] **Step 3: Implement config directory and auth modules**

`crates/odm-daemon/src/config.rs`:
```rust
//! Daemon configuration and token management.
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub bind_addr: String,
    pub token: String,
}

impl DaemonConfig {
    pub fn load_or_init(custom_config: Option<PathBuf>, custom_data: Option<PathBuf>, bind: Option<String>) -> Self {
        let (config_dir, data_dir) = match (custom_config, custom_data) {
            (Some(c), Some(d)) => (c, d),
            (c, d) => {
                let proj = directories::ProjectDirs::from("", "", "odm");
                let def_config = proj.as_ref().map(|p| p.config_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("./config"));
                let def_data = proj.as_ref().map(|p| p.data_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("./data"));
                (c.unwrap_or(def_config), d.unwrap_or(def_data))
            }
        };

        fs::create_dir_all(&config_dir).ok();
        fs::create_dir_all(&data_dir).ok();

        let token_path = config_dir.join("token");
        let token = if token_path.exists() {
            fs::read_to_string(&token_path).unwrap_or_default().trim().to_string()
        } else {
            let generated: String = (0..32).map(|_| format!("{:02x}", fastrand::u8(..))).collect();
            fs::write(&token_path, &generated).ok();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600));
            }
            generated
        };

        Self {
            config_dir,
            data_dir,
            bind_addr: bind.unwrap_or_else(|| "127.0.0.1:6820".to_string()),
            token,
        }
    }

    pub fn for_test(dir: &Path, token: &str) -> Self {
        Self {
            config_dir: dir.to_path_buf(),
            data_dir: dir.to_path_buf(),
            bind_addr: "127.0.0.1:0".to_string(),
            token: token.to_string(),
        }
    }
}
```

`crates/odm-daemon/src/auth.rs`:
Implement `require_bearer_token` and `validate_host_header` middleware.
Ensure `validate_host_header` inspects `Host`, strips ports, and permits `127.0.0.1`, `localhost`, `[::1]`. Reject any other host with HTTP 403 Forbidden and `ErrorResponse::new("invalid_host", "Host header is not allowed")`.
Ensure `require_bearer_token` bypasses `/api/v1/health` and `/health`, and for all other routes requires `Authorization: Bearer <token>`. Reject missing/invalid tokens with HTTP 401 Unauthorized and `ErrorResponse::new("unauthorized", "Missing or invalid authorization token")`.

- [x] **Step 4: Run auth tests and verify gate**

Run:
```bash
cargo test -p odm-daemon --test auth_and_security
```
Expected: `test result: ok. 3 passed; 0 failed`.

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-daemon Cargo.lock
git commit -m "feat(daemon): implement auth token, host checking and cors security"
```

---

### Task 4: Daemon REST API routes, background queue manager and WebSocket events

**Files:**
- Create: `crates/odm-daemon/src/state.rs`
- Create: `crates/odm-daemon/src/routes.rs`
- Create: `crates/odm-daemon/src/ws.rs`
- Create: `crates/odm-daemon/src/server.rs`
- Modify: `crates/odm-daemon/src/main.rs`
- Create: `crates/odm-daemon/tests/api_routes.rs`
- Create: `crates/odm-daemon/tests/websocket_events.rs`

**Interfaces:**
- Consumes: `odm-core`, `odm-api`, `axum`.
- Produces:
  - Full REST routes:
    - `GET /api/v1/health`
    - `POST /api/v1/probe`
    - `POST /api/v1/downloads` (creates job and starts background execution with queue semaphore)
    - `GET /api/v1/downloads` (filterable list)
    - `GET /api/v1/downloads/{id}` (detail with segments)
    - `POST /api/v1/downloads/{id}/pause`
    - `POST /api/v1/downloads/{id}/resume`
    - `POST /api/v1/downloads/{id}/cancel`
    - `POST /api/v1/downloads/{id}/retry`
    - `DELETE /api/v1/downloads/{id}` (with optional `delete_file` query)
    - `GET, POST, PATCH, DELETE /api/v1/queues`
    - `GET, PATCH /api/v1/settings`
  - WebSocket `/api/v1/events` stream with first-message token authentication.
  - Daemon startup and graceful exit if port already bound.

- [x] **Step 1: Write API routes and WebSocket tests first**

`crates/odm-daemon/tests/api_routes.rs`:
```rust
//! REST API routes test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::Request;
use axum::http::header::{ACCEPT_RANGES, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use odm_api::{
    AddDownloadRequest, AddDownloadResponse, DownloadDetailDto, HealthResponse,
    ListDownloadsResponse, ListQueuesResponse, QueueDto, SettingsDto,
};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_app;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tower::ServiceExt;

async fn test_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = vec![b'y'; 128 * 1024];

    let app = Router::new().route(
        "/sample.bin",
        get(move || {
            let p = payload.clone();
            async move {
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, p.len().to_string().leak()),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    p,
                )
                    .into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}/sample.bin")
}

#[tokio::test]
async fn full_rest_lifecycle_test() {
    let dir = tempdir().unwrap();
    let store = Store::open(&dir.path().join("daemon.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "secret");
    let state = AppState::new(engine, config);
    let app = build_app(state);

    let test_url = test_server().await;

    // 1. Health
    let req = axum::http::Request::builder()
        .uri("/api/v1/health")
        .header(HOST, "127.0.0.1")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Add download
    let add_body = serde_json::to_vec(&AddDownloadRequest {
        url: test_url,
        target_dir: Some(dir.path().to_string_lossy().to_string()),
        filename: Some("sample.bin".to_string()),
        queue: Some("default".to_string()),
        options: None,
    })
    .unwrap();

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .header(CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(add_body))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let add_resp: AddDownloadResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(add_resp.id, 1);

    // 3. List downloads
    let req = axum::http::Request::builder()
        .uri("/api/v1/downloads")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let list_resp: ListDownloadsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list_resp.downloads.len(), 1);
    assert_eq!(list_resp.downloads[0].id, 1);

    // 4. Get detail
    let req = axum::http::Request::builder()
        .uri("/api/v1/downloads/1")
        .header(HOST, "127.0.0.1")
        .header(AUTHORIZATION, "Bearer secret")
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let detail_resp: DownloadDetailDto = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(detail_resp.item.id, 1);
}
```

`crates/odm-daemon/tests/websocket_events.rs`:
```rust
//! WebSocket events streaming and authentication test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket};
use odm_api::{Event, WsAuthRequest};
use odm_core::{Engine, Store};
use odm_daemon::config::DaemonConfig;
use odm_daemon::server::build_app;
use odm_daemon::state::AppState;
use tempfile::tempdir;
use tokio::net::TcpListener;

#[tokio::test]
async fn websocket_authenticates_and_receives_events() {
    let dir = tempdir().unwrap();
    let store = Store::open(&dir.path().join("ws.sqlite")).unwrap();
    let engine = Engine::new(store).unwrap();
    let config = DaemonConfig::for_test(dir.path(), "ws-secret");
    let state = AppState::new(engine, config);
    let app = build_app(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Connect via tungstenite or reqwest
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/events"))
        .await
        .unwrap();

    use futures_util::{SinkExt, StreamExt};

    // First message: auth
    let auth = serde_json::to_string(&WsAuthRequest {
        token: "ws-secret".to_string(),
    })
    .unwrap();
    ws_stream
        .send(tokio_tungstenite::tungstenite::Message::Text(auth.into()))
        .await
        .unwrap();

    let resp = ws_stream.next().await.unwrap().unwrap();
    assert!(resp.to_text().unwrap().contains("authenticated"));
}
```

- [x] **Step 2: Implement `AppState`, handlers and router**

In `crates/odm-daemon/src/state.rs`:
- Holds `Arc<Engine>`, `DaemonConfig`, and a `Mutex<HashMap<String, Arc<Semaphore>>>` for queue concurrency limits.
- Provides helper methods `run_job(&self, id: JobId)` that acquires a permit from the job's queue semaphore and executes `self.engine.run(id)`.

In `crates/odm-daemon/src/routes.rs`:
- Implement handlers for:
  - `health`
  - `probe`
  - `add_download` (adds to engine and calls `state.spawn_download(id)`)
  - `list_downloads`
  - `get_download`
  - `pause_download`
  - `resume_download`
  - `cancel_download`
  - `retry_download`
  - `delete_download`
  - `list_queues`, `create_queue`, `update_queue`, `delete_queue`
  - `get_settings`, `update_settings`

In `crates/odm-daemon/src/ws.rs`:
- Implement `/api/v1/events` handler with WebSocket upgrade.
- Read first message, validate token, subscribe to engine events, and forward events as JSON text messages.

In `crates/odm-daemon/src/server.rs`:
- Construct `Router` with routes, security layers (Host checking, bearer auth, CORS), and error handler.

In `crates/odm-daemon/src/main.rs`:
- Parse CLI flags (`--bind`, `--config-dir`, `--data-dir`).
- Initialize tracing.
- If port already bound, log `"Failed to bind to {}: address already in use"` and exit with status 1.

- [x] **Step 3: Run daemon routes and ws tests**

Run:
```bash
cargo test -p odm-daemon --test api_routes
cargo test -p odm-daemon --test websocket_events
```
Expected: all pass.

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 4: Commit**

```bash
git add crates/odm-daemon Cargo.lock
git commit -m "feat(daemon): implement REST endpoints, queue manager and websocket events"
```

---

### Task 5: CLI client (`odm`) implementation

**Files:**
- Modify: `crates/odm-cli/Cargo.toml`
- Create: `crates/odm-cli/src/client.rs`
- Create: `crates/odm-cli/src/commands.rs`
- Create: `crates/odm-cli/src/format.rs`
- Modify: `crates/odm-cli/src/main.rs`
- Create: `crates/odm-cli/tests/cli_commands.rs`

**Interfaces:**
- Consumes: `clap`, `reqwest`, `odm-api`, `directories`.
- Produces: `odm` CLI binary with commands:
  - `odm add <url>...`
  - `odm list`
  - `odm info <id>`
  - `odm pause <id>`
  - `odm resume <id>`
  - `odm cancel <id>`
  - `odm retry <id>`
  - `odm remove <id>`
  - `odm queue <list|create|set-limit|delete>`
  - `odm daemon status`
  - Global flags: `--json`, `--server`, `--token`, `--config-dir`.

- [x] **Step 1: Write CLI command tests first**

`crates/odm-cli/tests/cli_commands.rs`:
```rust
//! CLI command parsing and client execution tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

#[test]
fn cli_help_exits_successfully() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open Download Manager"));
    assert!(stdout.contains("add"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("info"));
}

#[test]
fn daemon_status_when_offline_reports_offline() {
    let output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            "http://127.0.0.1:59999",
            "daemon",
            "status",
            "--json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("offline") || stdout.contains("error"));
}
```

- [x] **Step 2: Add dependencies in `crates/odm-cli/Cargo.toml`**

```toml
[dependencies]
clap = { version = "4.6.7", features = ["derive"] }
directories = "6.0.0"
odm-api = { path = "../odm-api" }
reqwest = { version = "0.13.5", default-features = false, features = ["rustls", "json"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
tokio = { version = "1.54.1", features = ["macros", "rt-multi-thread"] }
```

- [x] **Step 3: Implement client, formatting and CLI commands**

`crates/odm-cli/src/client.rs`:
- Implements `DaemonClient` communicating with daemon via `reqwest`.
- Reads auth token from `--token`, `ODM_TOKEN`, or `<config_dir>/token`.
- Methods: `health`, `add_download`, `list_downloads`, `get_download`, `pause`, `resume`, `cancel`, `retry`, `remove`, `list_queues`, `create_queue`, `set_queue_limit`, `delete_queue`.

`crates/odm-cli/src/format.rs`:
- Formats outputs cleanly as tabular text or prints raw JSON if `--json` is enabled.

`crates/odm-cli/src/commands.rs`:
- Defines `clap` structs and subcommands.

`crates/odm-cli/src/main.rs`:
- Executes parsed command and reports result or exits with status code 1 on error.

- [x] **Step 4: Run CLI unit tests and verify gate**

Run:
```bash
cargo test -p odm-cli --test cli_commands
```
Expected: `test result: ok. 2 passed; 0 failed`.

Run:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-cli Cargo.lock
git commit -m "feat(cli): implement odm command-line interface with json and table modes"
```

---

### Task 6: End-to-end integration test (`odm add <url>` downloads a real file)

**Files:**
- Create: `crates/odm-cli/tests/e2e_download.rs`

**Interfaces:**
- Consumes: compiled `odm` CLI and `odm-daemon` binaries.
- Produces: end-to-end integration test verifying the M3 milestone success criterion.

- [x] **Step 1: Write the end-to-end download test**

`crates/odm-cli/tests/e2e_download.rs`:
```rust
//! End-to-end integration test: `odm add <url>` downloads a real file via daemon.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use axum::extract::Request;
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tempfile::tempdir;
use tokio::net::TcpListener;

struct DaemonProcess(Child);

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start_test_file_server(data: Vec<u8>) -> (String, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route(
        "/largefile.bin",
        get(move || {
            let data = data.clone();
            async move {
                (
                    StatusCode::OK,
                    [
                        (CONTENT_TYPE, "application/octet-stream"),
                        (CONTENT_LENGTH, data.len().to_string().leak()),
                        (ACCEPT_RANGES, "bytes"),
                    ],
                    data,
                )
                    .into_response()
            }
        }),
    );

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}/largefile.bin"), addr)
}

#[tokio::test]
async fn odm_add_downloads_real_file_end_to_end() {
    let temp = tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    let download_dir = temp.path().join("downloads");
    fs::create_dir_all(&download_dir).unwrap();

    // 1. Generate 256 KiB random test payload
    let payload: Vec<u8> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
    let (url, _) = start_test_file_server(payload.clone()).await;

    // 2. Pick a random free port for the daemon
    let daemon_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let daemon_addr = daemon_listener.local_addr().unwrap();
    drop(daemon_listener);

    // 3. Spawn odm-daemon
    let child = Command::new(env!("CARGO_BIN_EXE_odm-daemon"))
        .args([
            "--bind",
            &daemon_addr.to_string(),
            "--config-dir",
            config_dir.to_str().unwrap(),
            "--data-dir",
            data_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let _guard = DaemonProcess(child);

    // 4. Poll health endpoint until daemon is responsive
    let client = reqwest::Client::new();
    let health_url = format!("http://{daemon_addr}/api/v1/health");
    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&health_url).send().await {
            if resp.status().is_success() {
                ready = true;
                break;
            }
        }
    }
    assert!(ready, "odm-daemon failed to start within timeout");

    // 5. Read generated token
    let token = fs::read_to_string(config_dir.join("token")).unwrap();
    let token = token.trim();

    // 6. Run `odm add <url>` via CLI
    let add_output = Command::new(env!("CARGO_BIN_EXE_odm"))
        .args([
            "--server",
            &format!("http://{daemon_addr}"),
            "--token",
            token,
            "add",
            &url,
            "--dir",
            download_dir.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();

    assert!(
        add_output.status.success(),
        "odm add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    // 7. Wait for file to finish downloading
    let downloaded_file = download_dir.join("largefile.bin");
    let mut finished = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if downloaded_file.exists() && !download_dir.join("largefile.bin.part").exists() {
            if let Ok(meta) = fs::metadata(&downloaded_file) {
                if meta.len() == payload.len() as u64 {
                    finished = true;
                    break;
                }
            }
        }
    }

    assert!(finished, "download did not complete in time");

    // 8. Verify binary contents match exactly byte-for-byte
    let actual_bytes = fs::read(&downloaded_file).unwrap();
    assert_eq!(actual_bytes, payload);
}
```

- [x] **Step 2: Run the end-to-end integration test**

Run:
```bash
cargo test -p odm-cli --test e2e_download
```
Expected: `test result: ok. 1 passed; 0 failed`.

- [x] **Step 3: Run the task checks and commit**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```
Expected: all pass.

```bash
git add crates/odm-cli/tests/e2e_download.rs Cargo.lock
git commit -m "test(cli): verify odm add downloads real file via daemon"
```

---

### Task 7: Final checks, changelog and milestone gate

**Files:**
- Modify: `CHANGELOG.md`

- [x] **Step 1: Record M3 capabilities in CHANGELOG.md**

Under `## [Unreleased]` and `### Added`, add:
```markdown
- Daemon REST API and WebSocket event streaming under `/api/v1` with bearer authentication, host header rebinding protection and CORS.
- Command-line client (`odm`) with human-readable and `--json` output modes for download management and daemon control.
```

- [x] **Step 2: Run the full M3 gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Expected: all pass; `cargo deny check` outputs:
`advisories ok, bans ok, licenses ok, sources ok`.

- [x] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: record M3 daemon API and CLI"
```

---

## Milestone Gate

M3 is complete when:
1. `cargo test --workspace` passes cleanly.
2. `cargo deny check` reports no license, advisory, or ban violations.
3. `crates/odm-cli/tests/e2e_download.rs` succeeds, proving that `odm add <url>` downloads a real file from a local server via the daemon API and CLI.
