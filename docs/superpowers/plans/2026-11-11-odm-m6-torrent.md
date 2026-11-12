# Milestone 6: Torrent Backend Implementation Plan

> **Specification Reference:** `docs/superpowers/specs/2026-11-11-odm-design.md` (Sections 2, 3, 4, 5, 6, 12).
> **Milestone Success Criterion:** A magnet link downloads and seeds (verified by automated tests across `odm-torrent`, `odm-core`, `odm-daemon`, `odm-cli`, and `odm-gui`, plus manual checklist `docs/manual-checklists/m6-torrent.md`).

---

## Architecture and Key Design Decisions

1. **`librqbit` as the torrent engine (`odm-torrent`)**:
   - One `librqbit::Session` lives inside the daemon process. It is created at daemon startup and shared across all torrent jobs.
   - `odm-torrent` owns the `TorrentBackend` struct that wraps the session and implements `odm_core::Backend`.
   - `librqbit` stores its own resume data in the data folder. `odm` stores only the mapping from `JobId` to `librqbit`'s torrent `Id` (an integer handle) in the `jobs` table as a JSON `backend_state` column.

2. **Magnet links, `.torrent` files, and HTTP `.torrent` URLs**:
   - `Source::Magnet(String)` and `Source::TorrentFile(PathBuf)` are already defined in `odm-core::job`.
   - The engine must detect backend kind during `add`: magnet URIs → `BackendKind::Torrent`, file paths ending in `.torrent` → `BackendKind::Torrent`, HTTP URLs whose probe returns `application/x-bittorrent` content-type → `BackendKind::Torrent`.
   - The daemon `add_download_handler` must accept magnet URIs and `.torrent` file paths, not just HTTP URLs.

3. **Probe returns the file list for selection**:
   - `TorrentBackend::probe` uses `librqbit` to resolve the torrent metadata (from magnet: wait for DHT metadata, from file/URL: parse immediately).
   - `ProbeResult` is extended with an optional `Vec<TorrentFileInfo>` field listing files (index, path, size) so the user can select which files to download.
   - For magnet links, probe may take up to 60 seconds waiting for metadata; it respects the control token for cancellation during this wait.

4. **File selection**:
   - `JobOptions` gains `selected_files: Option<Vec<u32>>` — indices of files to download. `None` means all files.
   - `AddTorrentOptions { only_files, .. }` in `librqbit` receives this selection.

5. **Speed limits**:
   - Applied through `librqbit`'s per-session or per-torrent rate limit options, not through `odm-core`'s token bucket.

6. **Seeding**:
   - After download completes, the job enters `JobState::Seeding`.
   - A background task monitors the upload ratio. When the ratio reaches the configured `seed_ratio` (default 1.0, configurable, 0 = skip seeding), the torrent is stopped and the job transitions to `Completed`.
   - The user can pause a seeding job (pauses the torrent) or cancel it (stops and removes the torrent from `librqbit`).

7. **Pause / Resume lifecycle**:
   - Pause fires the control token → the `run` method pauses the torrent via `librqbit` and returns `RunOutcome::Paused`.
   - Resume calls `run` again → the backend checks for a stored torrent handle and resumes it.

8. **Backend-specific resume state**:
   - SQLite migration 5 adds a `backend_state TEXT` column to the `jobs` table.
   - For torrent jobs this stores a JSON object: `{ "torrent_id": <u64> }`.
   - On daemon restart, `librqbit::Session` auto-recovers its own torrents from its persistence directory; the torrent backend re-associates `odm` jobs with `librqbit` handles using the stored torrent ID.

9. **Settings**:
   - New settings: `seed_ratio` (default `"1.0"`), `torrent_listen_port` (default `"6881"`).
   - Exposed through `GET/PATCH /api/v1/settings`.

---

## Dependency

| Crate | Dependency | Version | Purpose |
|---|---|---|---|
| `odm-torrent` | `librqbit` | `9.1.0` | Torrent protocol engine |
| `odm-torrent` | `odm-core` | path | Backend trait, job model |
| `odm-torrent` | `tokio` | `1.54.1` | Async runtime |
| `odm-torrent` | `serde` | `1.0.229` | JSON serialization for backend state |
| `odm-torrent` | `serde_json` | `1.0.154` | JSON serialization |
| `odm-torrent` | `thiserror` | `2.0.21` | Error types |
| `odm-torrent` | `tracing` | `0.1.44` | Logging |
| `odm-torrent` | `async-trait` | `0.1.92` | Backend trait impl |

---

## File Structure Plan

```
crates/odm-torrent/
  Cargo.toml              # Add dependencies
  src/
    lib.rs                # TorrentBackend, TorrentSession wrapper, Backend impl
    state.rs              # TorrentResumeState serde model
  tests/
    torrent_backend.rs    # Localhost seeder ↔ leecher integration tests
crates/odm-core/
  src/
    job.rs                # JobOptions: add selected_files, seed_ratio
    engine.rs             # Backend registry: select backend by source kind; support Seeding state
    store.rs              # Migration 5: backend_state column; persist/load backend state
    backend.rs            # BackendError: add Torrent variant
  tests/
    torrent_integration.rs # Engine-level torrent add/run test with TorrentBackend
crates/odm-api/
  src/
    download.rs           # JobOptionsDto: add selected_files, seed_ratio
    probe.rs              # ProbeResponse: add torrent_files field
    settings.rs           # SettingsDto: add seed_ratio, torrent_listen_port
crates/odm-daemon/
  src/
    state.rs              # Hold Arc<TorrentSession>; pass to Engine
    server.rs             # Register TorrentBackend with Engine
    routes.rs             # add_download_handler: accept magnet/torrent sources
  tests/
    torrent_api.rs        # API test: POST magnet link, list, pause, check seeding
crates/odm-cli/
  src/
    commands/add.rs       # Detect magnet: scheme; accept --torrent-file; --select-files
  tests/
    m6_torrent_cli.rs     # CLI torrent integration test
crates/odm-gui/
  src/
    ui/add_dialog.rs      # Torrent file list selection checkboxes after probe
    ui/sidebar.rs         # "Torrents" filter in sidebar
    state.rs              # Torrent-specific state (files, peers, seeding ratio)
  tests/
    m6_torrent_gui.rs     # GUI state tests for torrent flows
docs/manual-checklists/
  m6-torrent.md           # Manual tester checklist
CHANGELOG.md              # M6 additions
```

---

### Task 1: SQLite migration and backend-state persistence (`odm-core`)

**Files:**
- Modify: `crates/odm-core/src/store.rs`
- Modify: `crates/odm-core/src/job.rs`
- Create: `crates/odm-core/tests/backend_state_persistence.rs`

**Interfaces:**
- Produces: Migration 5 adding `backend_state TEXT` column to `jobs`, `Store::set_backend_state(id, json)`, `Store::get_backend_state(id) -> Option<String>`.
- Extends `JobOptions` with `selected_files: Option<Vec<u32>>` and `seed_ratio: Option<f64>`.

- [x] **Step 1: Write persistence tests first**

`crates/odm-core/tests/backend_state_persistence.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- Test `Store::set_backend_state` writes JSON and `Store::get_backend_state` reads it back.
- Test migration 5 applies cleanly on a fresh database.
- Test that `JobOptions` round-trips `selected_files` and `seed_ratio` through SQLite.

- [x] **Step 2: Implement migration 5 and Store methods**

In `crates/odm-core/src/store.rs`:
- Add `MIGRATION_5`:
  ```sql
  ALTER TABLE jobs ADD COLUMN backend_state TEXT;
  ```
- Implement `set_backend_state(id: JobId, state: &str) -> Result<(), StoreError>`.
- Implement `get_backend_state(id: JobId) -> Result<Option<String>, StoreError>`.
- Update `insert_job` and `load_job` to persist and load the `backend_state` column.

In `crates/odm-core/src/job.rs`:
- Add to `JobOptions`:
  ```rust
  pub selected_files: Option<Vec<u32>>,
  pub seed_ratio: Option<f64>,
  ```

- [x] **Step 3: Run tests and verify gate**

```bash
cargo test -p odm-core --test backend_state_persistence
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 4: Commit**

```bash
git add crates/odm-core Cargo.lock
git commit -m "feat(core): add backend_state persistence and torrent job options"
```

---

### Task 2: Extend Backend trait error and Engine backend selection (`odm-core`)

**Files:**
- Modify: `crates/odm-core/src/backend.rs`
- Modify: `crates/odm-core/src/engine.rs`
- Create: `crates/odm-core/tests/backend_selection.rs`

**Interfaces:**
- Adds `BackendError::Torrent(String)` variant.
- Extends `Engine` with a backend registry: `Engine::register_backend(Box<dyn Backend>)`.
- `Engine::add` becomes `Engine::add_download` accepting `Source` directly (not just URL string), selecting backend by source type.
- `Engine::run` selects the appropriate backend from the registry by `BackendKind`.

> **Note:** The existing `Engine::add(&self, url, target_dir, options)` method is kept for backward compatibility but internally delegates to `add_download`. Per AGENTS.md rule 9, the `Backend` trait itself is NOT changed — only `BackendError` and `Engine` are modified.

- [x] **Step 1: Write backend selection tests first**

`crates/odm-core/tests/backend_selection.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- Test `Engine::add_download(Source::Magnet(...), ...)` creates a job with `BackendKind::Torrent`.
- Test `Engine::add_download(Source::TorrentFile(...), ...)` creates a job with `BackendKind::Torrent`.
- Test `Engine::add_download(Source::Url("https://..."), ...)` creates a job with `BackendKind::Http`.

- [x] **Step 2: Add `BackendError::Torrent` and extend Engine**

In `crates/odm-core/src/backend.rs`:
- Add variant:
  ```rust
  #[error("torrent error: {0}")]
  Torrent(String),
  ```
- `Torrent(String)` is retryable.

In `crates/odm-core/src/engine.rs`:
- Add a `backends: HashMap<BackendKind, Arc<dyn Backend>>` field to `Engine`.
- `Engine::new` registers `HttpBackend` under `BackendKind::Http`.
- Add `Engine::register_backend(&mut self, backend: Arc<dyn Backend>)`.
- Add `Engine::add_download(&self, source: Source, target_dir: PathBuf, options: JobOptions) -> Result<Job, EngineError>`:
  - `Source::Magnet(_)` | `Source::TorrentFile(_)` → `BackendKind::Torrent`.
  - `Source::Url(_)` → `BackendKind::Http` (existing logic).
- Existing `Engine::add` delegates to `add_download(Source::Url(...), ...)`.
- `Engine::run` looks up the backend from the registry by `job.backend()` instead of always using `self.backend`.
- Add `EngineError::NoBackend(BackendKind)` variant for when a backend is not registered.

- [x] **Step 3: Run tests and verify gate**

```bash
cargo test -p odm-core --test backend_selection
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 4: Commit**

```bash
git add crates/odm-core
git commit -m "feat(core): add backend registry and source-based backend selection"
```

---

### Task 3: Implement TorrentBackend (`odm-torrent`)

**Files:**
- Modify: `crates/odm-torrent/Cargo.toml`
- Rewrite: `crates/odm-torrent/src/lib.rs`
- Create: `crates/odm-torrent/src/state.rs`
- Create: `crates/odm-torrent/tests/torrent_backend.rs`

**Interfaces:**
- Consumes: `librqbit = "9.1.0"`, `odm-core`, `tokio`, `serde`, `serde_json`, `thiserror`, `tracing`, `async-trait`.
- Produces: `TorrentBackend` implementing `Backend`, `TorrentSession` wrapping `librqbit::Session`.

- [x] **Step 1: Write torrent backend integration test first**

`crates/odm-torrent/tests/torrent_backend.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- **Seeder-leecher test on localhost:**
  1. Create a temp directory with a small generated file (e.g. 64 KiB of random bytes).
  2. Start a `librqbit::Session` as a seeder: add the file as a torrent, get the magnet URI.
  3. Create a `TorrentBackend` with a second `librqbit::Session` (different port, different temp dir).
  4. Call `probe` with the magnet source → assert `ProbeResult` contains the filename and file size.
  5. Create a `JobContext` and call `run` → assert `RunOutcome::Completed`.
  6. Verify the downloaded file hash matches the source file hash.
- **Pause test:**
  1. Same seeder setup.
  2. Start `run` and immediately fire `control.pause()`.
  3. Assert `RunOutcome::Paused`.
- **Cancel test:**
  1. Same seeder setup.
  2. Start `run` and immediately fire `control.cancel()`.
  3. Assert `RunOutcome::Cancelled`.

- [x] **Step 2: Add dependencies to `crates/odm-torrent/Cargo.toml`**

```toml
[dependencies]
async-trait = "0.1.92"
librqbit = "9.1.0"
odm-core = { path = "../odm-core" }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
thiserror = "2.0.21"
tokio = { version = "1.54.1", features = ["sync", "time", "macros"] }
tracing = "0.1.44"

[dev-dependencies]
sha2 = "0.10.8"
tempfile = "3.27.0"
tokio = { version = "1.54.1", features = ["rt-multi-thread", "test-util", "macros"] }

[lints]
workspace = true
```

Run `cargo deny check`.

- [x] **Step 3: Implement TorrentBackend**

In `crates/odm-torrent/src/state.rs`:
```rust
use serde::{Deserialize, Serialize};

/// Persisted state mapping an odm job to a librqbit torrent handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentResumeState {
    pub torrent_id: usize,
}
```

In `crates/odm-torrent/src/lib.rs`:
```rust
#![forbid(unsafe_code)]
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use librqbit::{AddTorrent, AddTorrentOptions, Session};
use odm_core::backend::{
    Backend, BackendError, JobContext, RunOutcome,
};
use odm_core::job::{BackendKind, JobOptions, ProbeResult, Source};

pub struct TorrentBackend {
    session: Arc<Session>,
    default_seed_ratio: f64,
}
```

Key implementation details:
- `TorrentBackend::new(download_dir: PathBuf, listen_port: u16) -> Result<Self, BackendError>`:
  Creates the `librqbit::Session`.
- `probe`:
  - `Source::Magnet(uri)`: Use `session.add_torrent(AddTorrent::from_url(&uri), Some(AddTorrentOptions { paused: true, .. }))`, wait for metadata, extract file list, then pause/remove.
  - `Source::TorrentFile(path)`: Parse the `.torrent` file locally.
  - Returns `ProbeResult` with `backend: BackendKind::Torrent`, the torrent name, total size, and file list.
- `run`:
  - Add (or resume) the torrent in `librqbit`.
  - Poll progress every 500ms and report via `ctx.progress.report(...)`.
  - Monitor `ctx.control` for pause/cancel.
  - When download completes and `seed_ratio > 0.0`, transition the odm job to `Seeding` (via store), monitor upload ratio, and stop when ratio is met.
  - On pause: pause the torrent in `librqbit`, save `TorrentResumeState` to store, return `Paused`.
  - On cancel: remove the torrent from `librqbit`, return `Cancelled`.

- [x] **Step 4: Run torrent backend tests and verify gate**

```bash
cargo test -p odm-torrent --test torrent_backend
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-torrent Cargo.lock
git commit -m "feat(torrent): implement torrent backend with librqbit session"
```

---

### Task 4: Wire TorrentBackend into daemon and extend API (`odm-daemon`, `odm-api`)

**Files:**
- Modify: `crates/odm-api/src/download.rs`
- Modify: `crates/odm-api/src/probe.rs`
- Modify: `crates/odm-api/src/settings.rs`
- Modify: `crates/odm-daemon/Cargo.toml`
- Modify: `crates/odm-daemon/src/state.rs`
- Modify: `crates/odm-daemon/src/server.rs`
- Modify: `crates/odm-daemon/src/routes.rs`
- Create: `crates/odm-daemon/tests/torrent_api.rs`

**Interfaces:**
- `odm-api`: `ProbeResponse` gains `torrent_files: Option<Vec<TorrentFileDto>>`. `JobOptionsDto` gains `selected_files`, `seed_ratio`. `SettingsDto` gains `seed_ratio`, `torrent_listen_port`.
- `odm-daemon`: `AppState` holds `Arc<TorrentBackend>`. `Engine` gets the torrent backend registered at startup. `add_download_handler` detects magnet URIs and `.torrent` sources. `probe_handler` forwards to the torrent backend when the URL is a magnet. Settings routes serve `seed_ratio` and `torrent_listen_port`.

- [x] **Step 1: Write daemon torrent API tests first**

`crates/odm-daemon/tests/torrent_api.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- Test `POST /api/v1/downloads` with `{"url": "magnet:?xt=urn:btih:..."}` returns 200 and creates a torrent job.
- Test `GET /api/v1/downloads/{id}` shows `backend: "torrent"`.
- Test `POST /api/v1/probe` with a magnet URI returns `backend: "torrent"` and a `torrent_files` list.
- Test `PATCH /api/v1/settings` with `seed_ratio` and `torrent_listen_port` persists and reads back.

> **Note:** These tests use the localhost seeder pattern from Task 3 to have a real torrent to interact with.

- [x] **Step 2: Extend API types**

In `crates/odm-api/src/probe.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TorrentFileDto {
    pub index: u32,
    pub path: String,
    pub size: u64,
}
```
Add to `ProbeResponse`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub torrent_files: Option<Vec<TorrentFileDto>>,
```

In `crates/odm-api/src/download.rs`, add to `JobOptionsDto`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub selected_files: Option<Vec<u32>>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub seed_ratio: Option<f64>,
```

In `crates/odm-api/src/settings.rs`, add to `SettingsDto`:
```rust
pub seed_ratio: f64,
pub torrent_listen_port: u16,
```
Add to `UpdateSettingsRequest`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub seed_ratio: Option<f64>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub torrent_listen_port: Option<u16>,
```

- [x] **Step 3: Wire backend into daemon**

In `crates/odm-daemon/Cargo.toml`:
- Add `odm-torrent = { path = "../odm-torrent" }`.

In `crates/odm-daemon/src/state.rs`:
- Import and hold `TorrentBackend` in `AppState`.

In `crates/odm-daemon/src/server.rs`:
- At startup, read `seed_ratio` and `torrent_listen_port` from settings store.
- Create `TorrentBackend::new(data_dir, listen_port)`.
- Register the torrent backend with `engine.register_backend(...)`.

In `crates/odm-daemon/src/routes.rs`:
- `add_download_handler`: detect `magnet:` scheme or `.torrent` file path in `req.url`, build the appropriate `Source` variant, call `engine.add_download(source, ...)`.
- `probe_handler`: detect `magnet:` scheme, delegate to torrent backend probe.
- `get_settings_handler` / `update_settings_handler`: handle `seed_ratio`, `torrent_listen_port`.

- [x] **Step 4: Run tests and verify gate**

```bash
cargo test -p odm-daemon --test torrent_api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-api crates/odm-daemon Cargo.lock
git commit -m "feat(daemon): wire torrent backend into daemon and extend api"
```

---

### Task 5: CLI torrent support (`odm-cli`)

**Files:**
- Modify: `crates/odm-cli/src/commands/add.rs`
- Modify: `crates/odm-cli/src/main.rs`
- Create: `crates/odm-cli/tests/m6_torrent_cli.rs`

**Interfaces:**
- `odm add <magnet-uri>` detects `magnet:` and sends it as the URL.
- `odm add --torrent-file <path>` reads the `.torrent` file path and sends it.
- `odm add ... --select-files 0,1,3` passes selected file indices.
- `odm add ... --seed-ratio 2.0` overrides the default seed ratio.

- [x] **Step 1: Write CLI torrent tests first**

`crates/odm-cli/tests/m6_torrent_cli.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- Test CLI `odm add "magnet:?xt=..."` sends a request with the magnet URI and receives a success response.
- Test CLI `odm add --torrent-file ./test.torrent` sends the torrent file path.
- Test CLI `odm add "magnet:..." --select-files 0,2 --seed-ratio 0.5` passes options correctly.

- [x] **Step 2: Implement CLI torrent flags**

In `crates/odm-cli/src/commands/add.rs`:
- Detect `magnet:` scheme in the URL argument.
- Add `--torrent-file <PATH>` flag.
- Add `--select-files <INDICES>` flag (comma-separated u32 list).
- Add `--seed-ratio <RATIO>` flag (f64).
- Populate `AddDownloadRequest` with `selected_files` and `seed_ratio` in `options`.

- [x] **Step 3: Run CLI tests and verify gate**

```bash
cargo test -p odm-cli --test m6_torrent_cli
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 4: Commit**

```bash
git add crates/odm-cli
git commit -m "feat(cli): add magnet, torrent-file, select-files and seed-ratio flags"
```

---

### Task 6: GUI torrent support (`odm-gui`)

**Files:**
- Modify: `crates/odm-gui/src/messages.rs`
- Modify: `crates/odm-gui/src/state.rs`
- Modify: `crates/odm-gui/src/bridge.rs`
- Modify: `crates/odm-gui/src/ui/add_dialog.rs`
- Modify: `crates/odm-gui/src/ui/sidebar.rs`
- Create: `crates/odm-gui/tests/m6_torrent_gui.rs`

**Interfaces:**
- Add dialog: when a magnet URI or `.torrent` file is entered, probe returns a file list. Show checkboxes for each file. Show seed ratio override field.
- Sidebar: "Torrents" filter shows jobs with `backend == "torrent"`.
- Job detail panel: show peers connected (from progress events), upload ratio, seeding status.

- [x] **Step 1: Write GUI torrent state tests first**

`crates/odm-gui/tests/m6_torrent_gui.rs`:
```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]
```
- Test add dialog state: when probe response contains `torrent_files`, the state holds the file list and all files are selected by default.
- Test toggling file selection updates `selected_files` in the request.
- Test sidebar filter "Torrents" filters to `backend == "torrent"` jobs.
- Test seeding state display: when job state is `seeding`, the UI state shows seeding progress.

- [x] **Step 2: Implement GUI torrent components**

In `crates/odm-gui/src/ui/add_dialog.rs`:
- After probe, if `torrent_files` is `Some`, render a scrollable file list with checkboxes.
- Add a seed ratio input field (defaults to the global setting).

In `crates/odm-gui/src/ui/sidebar.rs`:
- Add "Torrents" to the filter list, matching `backend == "torrent"`.

In `crates/odm-gui/src/state.rs`:
- Add `torrent_files: Vec<TorrentFileDto>` and `selected_file_indices: HashSet<u32>` to `AddDialogState`.
- Track seeding ratio in job display state.

In `crates/odm-gui/src/bridge.rs`:
- Map `selected_files` into `AddDownloadRequest.options`.

- [x] **Step 3: Run GUI tests and verify gate**

```bash
cargo test -p odm-gui --test m6_torrent_gui
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 4: Commit**

```bash
git add crates/odm-gui
git commit -m "feat(gui): add torrent file selection, seeding display and sidebar filter"
```

---

### Task 7: End-to-end integration, manual checklist, changelog and milestone gate

**Files:**
- Create: `docs/manual-checklists/m6-torrent.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/superpowers/plans/2026-11-11-odm-m6-torrent.md` (mark tasks done)

- [x] **Step 1: Create manual checklist in `docs/manual-checklists/m6-torrent.md`**

Document all manual verification steps:
1. Start daemon. Add a magnet link from CLI → observe probing, file list display, download, seeding, completion.
2. Add a `.torrent` file from CLI → same lifecycle.
3. Add a magnet link from GUI → file selection dialog, selective download, seeding ratio display.
4. Pause a downloading torrent → verify `Paused` state, resume → verify it continues.
5. Cancel a seeding torrent → verify `Cancelled` state, torrent removed from `librqbit`.
6. Set `seed_ratio` to `0` → verify torrent completes immediately without seeding.
7. Set `seed_ratio` to `2.0` → verify torrent seeds to ratio 2.0 then completes.
8. Restart daemon while a torrent is downloading → verify it recovers and resumes.

- [x] **Step 2: Record M6 capabilities in CHANGELOG.md**

Under `## [Unreleased]` and `### Added`:
```markdown
- Torrent backend via librqbit: magnet links and .torrent files download through the same interface as HTTP.
- Torrent file selection: choose which files to download from multi-file torrents.
- Configurable seeding: seed to a target ratio (default 1.0), then auto-complete. Set ratio to 0 to skip seeding.
- Torrent settings: listen port and seed ratio configurable from CLI, GUI, and API.
- Torrent sidebar filter in GUI.
- DHT, peer exchange, and UPnP port mapping via librqbit.
- Torrent resume: torrents survive daemon restarts and continue where they left off.
```

- [x] **Step 3: Run full milestone gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Expected: All pass cleanly; zero clippy warnings; zero unsafe; deny checks green.

- [x] **Step 4: Commit**

```bash
git add docs/manual-checklists/m6-torrent.md CHANGELOG.md docs/superpowers/plans/2026-11-11-odm-m6-torrent.md
git commit -m "docs: record M6 torrent backend"
```
