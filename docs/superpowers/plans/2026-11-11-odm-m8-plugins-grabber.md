# Milestone 8: Plugins and Site Grabber Implementation Plan

> **Specification Reference:** `docs/superpowers/specs/2026-11-11-odm-design.md` (Sections 2, 3, 4, 5, 6, 8, 12).  
> **Milestone Success Criterion:** A sample plugin resolves a URL; a grab lists files (verified by automated tests across `odm-core`, `odm-plugin`, `odm-api`, `odm-daemon`, `odm-cli`, and `odm-gui`, plus manual checklist `docs/manual-checklists/m8-plugins-grabber.md`).

---

## Architecture and Key Design Decisions

1. **Site Grabber (`odm-core::grabber`)**:
   - Given a start URL, crawl pages on the same host up to a configured depth limit (e.g., depth 1 = start page links, depth 2 = links from those pages).
   - Same-host restriction: `page_url.host_str() == start_url.host_str()`. External links are ignored for crawling.
   - `robots.txt` support: Enabled by default (`respect_robots_txt: true`). Fetches `{scheme}://{host}:{port}/robots.txt` if available; parses rules for User-agent `odm` and wildcard `*`. Disallowed paths are skipped.
   - HTML parsing: Uses `scraper = { version = "0.29.0", default-features = false }` to parse DOM and extract links from `<a href>`, `<img src>`, `<source src>`, `<video src>`, `<audio src>`, and `<link href>`.
   - Resolves all relative URLs against the page's base URL (`base_url.join(attr_val)`).
   - Filtering: Candidate links are filtered by file extensions (e.g. `["zip", "png", "jpg"]`, case-insensitive) and/or wildcard/pattern filters.
   - Deduplication: Unique URLs collected in order of discovery.
   - Safety guard rails: Configurable `max_pages` (default 50) and cancellation support to prevent infinite crawling.

2. **External-Process Plugin Host (`odm-plugin`)**:
   - Plugins live in subdirectories of `<data_dir>/plugins/<plugin_name>/`.
   - Each plugin contains a `plugin.toml` manifest:
     ```toml
     [plugin]
     name = "sample-resolver"
     version = "0.1.0"
     description = "Example resolver plugin"
     role = "resolver"
     url_patterns = ["*://*.example.com/watch/*", "^https?://example\\.com/item/.*$"]
     command = ["python", "resolver.py"]
     ```
   - Manifest parser supports wildcard globbing (`*`, `?`) converted to regex, or raw regex patterns.
   - Resolver JSON Protocol:
     - Input (stdin):
       ```json
       {
         "version": "0.1",
         "url": "https://example.com/watch/12345"
       }
       ```
     - Output (stdout):
       ```json
       {
         "downloads": [
           {
             "url": "https://cdn.example.com/stream.mp4",
             "headers": {
               "Referer": "https://example.com/",
               "Authorization": "Bearer ..."
             },
             "filename": "stream.mp4"
           }
         ]
       }
       ```
   - Timeout and fault isolation: A plugin has 30 seconds to answer (`tokio::time::timeout`). If it hangs, the child process is terminated (SIGTERM / `taskkill` on Windows). Crashes (non-zero exit) and invalid JSON produce typed errors (`PluginError`) and do not crash or block the daemon.
   - Security: Plugins run as regular processes under the user's account without sandboxing. The API, CLI, and GUI clearly display the security notice.

3. **Plugin Backend (`odm-plugin::PluginBackend`)**:
   - Implements `odm_core::backend::Backend` with `kind() -> BackendKind::Plugin`.
   - `probe`: Invokes the plugin resolver with the source URL. Takes the primary resolved direct download URL, queries it via HTTP HEAD/GET to verify size and range support, and returns `ProbeResult`.
   - `run`: Resolves the URL (or reads saved resolved download info from `backend_state`) and runs the HTTP download transfer engine, supporting segmentation, resume, rate limiting, and cancellation.

4. **Daemon Integration (`odm-daemon`)**:
   - Discovers installed plugins in `<data_dir>/plugins` at startup and registers `PluginBackend` with the `Engine`.
   - Backend selection update: In `POST /api/v1/downloads` and `POST /api/v1/probe`, if the URL matches an installed plugin pattern, select `BackendKind::Plugin`.
   - `GET /api/v1/plugins`: Returns `PluginListResponse` listing installed plugins.
   - `POST /api/v1/grabber`: Runs `SiteGrabber` and returns `GrabberResponse`.

5. **CLI & GUI Integration (`odm-cli`, `odm-gui`)**:
   - CLI:
     - `odm grab <url> [--depth <n>] [--ext <ext>...] [--pattern <pat>] [--no-robots] [--max-pages <n>] [--add] [--json]`
     - `odm plugin list [--json]`
   - GUI:
     - Grabber Dialog: accessible via `[🌐 Grabber]` on the toolbar. Provides inputs, progress indicator, results list with checkboxes, and an "Add Selected" batch action.
     - Plugins Screen: displays installed plugins and features the prominent security notice:
       `⚠️ Security Notice: Plugins are external programs running with your full user permissions. There is no sandbox. Only install plugins from sources you trust.`

---

## Dependencies Plan

| Crate | Dependency | Version | Purpose |
|---|---|---|---|
| `odm-core` | `scraper` | `0.29.0` (default-features = false) | HTML DOM parsing and CSS selectors for Site Grabber |
| `odm-plugin` | `toml` | `0.8.20` | Parsing `plugin.toml` manifests |
| `odm-plugin` | `regex` | `1.13.1` | Matching URL patterns in plugin manifests |
| `odm-plugin` | `tokio` | `1.54.1` (features: `["process", "io-util", "time", "sync"]`) | Child process spawning, stdin/stdout piping, 30s timeout |
| `odm-plugin` | `serde` | `1.0.229` (features: `["derive"]`) | JSON & TOML serialization |
| `odm-plugin` | `serde_json` | `1.0.154` | JSON stdin/stdout protocol serialization |
| `odm-plugin` | `thiserror` | `2.0.21` | Typed error handling |
| `odm-plugin` | `tracing` | `0.1.44` | Diagnostic logging |
| `odm-plugin` | `async-trait` | `0.1.92` | Implementing `Backend` trait |
| `odm-plugin` | `reqwest` | `0.13.5` (default-features = false, features = `["rustls"]`) | Probing resolved HTTP endpoints |
| `odm-plugin` | `odm-core` | `path = "../odm-core"` | Core domain models, `Backend` trait, HTTP transfer engine |
| `odm-daemon` | `odm-plugin` | `path = "../odm-plugin"` | Plugin discovery, registry, and backend wiring |

---

## Tasks

| Task | Status | Commit | Description |
|---|---|---|---|
| Task 1 | [x] Completed | `3e1f65b` | `feat(core): implement site grabber with robots.txt and html link extraction` |
| Task 2 | [x] Completed | `89f8cac` | `feat(plugin): implement plugin manifest parser and url pattern matcher` |
| Task 3 | [x] Completed | `6d415f0` | `feat(plugin): implement plugin host process execution with timeout and fault isolation` |
| Task 4 | [x] Completed | `e3b4cf2` | `feat(plugin): implement plugin registry and plugin backend` |
| Task 5 | [x] Completed | `0350006` | `feat(api): add grabber and plugin request and response dtos` |
| Task 6 | [x] Completed | `454a258` | `feat(daemon): wire plugin registry, list endpoint and plugin backend selection` |
| Task 7 | [x] Completed | `44dffed` | `feat(daemon): implement post grabber api endpoint` |
| Task 8 | [x] Completed | `0bbbc6a` | `feat(cli): add odm grab and odm plugin list commands` |
| Task 9 | [x] Completed | `3ff3145` | `feat(gui): implement grabber dialog and plugin list view with security notice` |
| Task 10 | [x] Completed | Pending | `docs: record M8 plugins and site grabber` |

### Task 1: Site Grabber Core Engine (`odm-core`) [Completed]

- **Commit Message:** `feat(core): implement site grabber with robots.txt and html link extraction`
- **Goal:** Add `scraper` to `odm-core`, add `BackendError::Plugin` variant, implement `RobotsTxt` rule matcher, and implement `SiteGrabber` crawling pages on the same host up to depth limit with extension/pattern filtering.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-core/tests/grabber.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Sets up a local `axum` test server with:
     - `/robots.txt` disallowing `/secret/`
     - `/index.html` containing links to `/page2.html`, `/secret/file.zip`, `/image1.png`, `/docs/report.pdf`, and external link `https://other.com/ext.zip`
     - `/page2.html` containing links to `/image2.jpg` and `/deep/page3.html`
   - Test 1: Crawl depth 1 with extension filter `["png", "pdf"]` respects robots.txt and collects `image1.png` and `report.pdf`, ignoring `/secret/file.zip` and `other.com`.
   - Test 2: Crawl depth 2 collects `image1.png`, `report.pdf`, and `image2.jpg`.
   - Test 3: Disabling robots.txt (`respect_robots_txt: false`) collects `/secret/file.zip`.
   - Test 4: Wildcard/regex pattern filter collects only matching URLs.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-core --test grabber
   ```
   *Expected:* Compilation error (`SiteGrabber` or `grabber` not found).
3. **Implement Code:**
   - Add `scraper = { version = "0.29.0", default-features = false }` to `crates/odm-core/Cargo.toml`.
   - Run `cargo deny check` to ensure licenses and advisories pass.
   - In `crates/odm-core/src/backend.rs`:
     - Add `#[error("plugin error: {0}")] Plugin(String)` to `BackendError`.
   - Create `crates/odm-core/src/grabber.rs`:
     - Implement `RobotsTxt`: parses `robots.txt` lines, checks `is_allowed(user_agent, path)`.
     - Implement `GrabberConfig`, `GrabbedFile`, `GrabResult`, `GrabberError`.
     - Implement `SiteGrabber`:
       - `crawl(&self, client: &reqwest::Client) -> Result<GrabResult, GrabberError>`
       - Enforces same-host rule (`url.host_str() == start_url.host_str()`).
       - Enforces depth limit and `max_pages`.
       - Extracts `<a href>`, `<img src>`, `<source src>`, `<video src>`, `<audio src>`, `<link href>` using `scraper::Html` and `scraper::Selector`.
       - Resolves relative URLs with `base_url.join(...)`.
       - Applies extension filter (case-insensitive) and pattern filter.
   - In `crates/odm-core/src/lib.rs`: export `pub mod grabber;`.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-core --test grabber
   ```
   *Expected:* All grabber tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-core/Cargo.toml Cargo.lock crates/odm-core/src/ crates/odm-core/tests/grabber.rs
   git commit -m "feat(core): implement site grabber with robots.txt and html link extraction"
   ```

---

### Task 2: Plugin Manifest and Pattern Matching (`odm-plugin`) [Completed]

- **Commit Message:** `feat(plugin): implement plugin manifest parser and url pattern matcher`
- **Goal:** Implement `plugin.toml` manifest loading and URL pattern matching (wildcard and regex).

#### Steps:
1. **Write Test First:**
   Create `crates/odm-plugin/tests/plugin_manifest.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: Parse valid `plugin.toml` with `name`, `version`, `role = "resolver"`, `url_patterns`, `command`.
   - Test 2: Wildcard pattern matching (`*://example.com/watch/*`, `*.example.org/*`) matches expected URLs and rejects non-matching URLs.
   - Test 3: Regex pattern matching (`^https?://(www\\.)?mysite\\.com/item/\\d+$`) matches correctly.
   - Test 4: Manifest with invalid role (e.g. `role = "uploader"`) returns validation error.
   - Test 5: Manifest missing required fields returns error.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-plugin --test plugin_manifest
   ```
   *Expected:* Compilation error.
3. **Implement Code:**
   - Update `crates/odm-plugin/Cargo.toml` with:
     ```toml
     toml = "0.8.20"
     regex = "1.13.1"
     serde = { version = "1.0.229", features = ["derive"] }
     serde_json = "1.0.154"
     thiserror = "2.0.21"
     ```
   - In `crates/odm-plugin/src/error.rs`:
     - Define `PluginError` enum (`InvalidManifest`, `UnsupportedRole`, `ProcessFailed`, `Timeout`, `InvalidJson`, etc.).
   - In `crates/odm-plugin/src/manifest.rs`:
     - Define `PluginManifest`, `PluginRole`, and `UrlPattern`.
     - Implement `PluginManifest::from_toml_str(content: &str) -> Result<Self, PluginError>`.
     - Implement `PluginManifest::matches_url(&self, url: &str) -> bool`.
   - In `crates/odm-plugin/src/lib.rs`: export modules.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-plugin --test plugin_manifest
   ```
   *Expected:* All manifest tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-plugin/Cargo.toml Cargo.lock crates/odm-plugin/src/ crates/odm-plugin/tests/plugin_manifest.rs
   git commit -m "feat(plugin): implement plugin manifest parser and url pattern matcher"
   ```

---

### Task 3: Plugin Host Execution and Protocol (`odm-plugin`) [Completed]

- **Commit Message:** `feat(plugin): implement plugin host process execution with timeout and fault isolation`
- **Goal:** Implement `PluginHost` that spawns plugin executables, sends JSON on stdin, parses JSON on stdout, enforces a 30s timeout, and isolates crashes/invalid output.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-plugin/tests/plugin_host.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Build a fake test helper binary/script for testing:
     - Normal case: receives JSON on stdin, outputs valid JSON with downloads.
     - Timeout case: sleeps past the timeout duration.
     - Crash case: exits with code 1 and writes to stderr.
     - Malformed case: outputs invalid JSON.
   - Test 1: Successful execution parses `ResolverResponse`.
   - Test 2: Process exceeding timeout is killed and returns `PluginError::Timeout`.
   - Test 3: Non-zero exit code returns `PluginError::ProcessFailed` with stderr message.
   - Test 4: Malformed JSON output returns `PluginError::InvalidJson`.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-plugin --test plugin_host
   ```
   *Expected:* Compilation error (`PluginHost` not found).
3. **Implement Code:**
   - Update `crates/odm-plugin/Cargo.toml` with:
     ```toml
     tokio = { version = "1.54.1", features = ["process", "io-util", "time", "sync"] }
     tracing = "0.1.44"
     ```
   - In `crates/odm-plugin/src/protocol.rs`:
     - Define `ResolverRequest`: `version: String`, `url: String`.
     - Define `ResolvedDownload`: `url: String`, `headers: HashMap<String, String>`, `filename: Option<String>`.
     - Define `ResolverResponse`: `downloads: Vec<ResolvedDownload>`.
   - In `crates/odm-plugin/src/host.rs`:
     - Implement `PluginHost::resolve(manifest: &PluginManifest, plugin_dir: &Path, url: &str, timeout_dur: Duration) -> Result<ResolverResponse, PluginError>`.
     - Uses `tokio::process::Command` with piped stdin/stdout/stderr.
     - Cross-platform Windows handling: if command ends in `.cmd` or `.bat`, routes through `cmd.exe /C`.
     - Wraps execution in `tokio::time::timeout`.
     - Terminates child process upon timeout.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-plugin --test plugin_host
   ```
   *Expected:* All host tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-plugin/Cargo.toml Cargo.lock crates/odm-plugin/src/ crates/odm-plugin/tests/plugin_host.rs
   git commit -m "feat(plugin): implement plugin host process execution with timeout and fault isolation"
   ```

---

### Task 4: Plugin Registry and Backend Implementation (`odm-plugin`) [Completed]

- **Commit Message:** `feat(plugin): implement plugin registry and plugin backend`
- **Goal:** Implement `PluginRegistry` to discover plugins from `<data_dir>/plugins`, and `PluginBackend` implementing `Backend`.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-plugin/tests/plugin_backend.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Sets up a temp directory containing a mock plugin folder with `plugin.toml` and mock executable.
   - Sets up a local `axum` test server serving the resolved file.
   - Test 1: `PluginRegistry::discover` discovers the installed plugin and matches a test URL.
   - Test 2: `PluginBackend::probe` resolves URL via mock plugin and probes resolved file on test server, returning `ProbeResult` with size and filename.
   - Test 3: `PluginBackend::run` completes the download and verifies content matches.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-plugin --test plugin_backend
   ```
   *Expected:* Compilation error.
3. **Implement Code:**
   - Update `crates/odm-plugin/Cargo.toml`:
     ```toml
     async-trait = "0.1.92"
     odm-core = { path = "../odm-core" }
     reqwest = { version = "0.13.5", default-features = false, features = ["rustls"] }
     ```
   - In `crates/odm-plugin/src/registry.rs`:
     - Implement `PluginRegistry::new(plugins_dir: PathBuf)`.
     - Implement `discover(&mut self) -> Result<usize, PluginError>`.
     - Implement `find_for_url(&self, url: &str) -> Option<(&PluginManifest, PathBuf)>`.
     - Implement `list(&self) -> Vec<PluginInfo>`.
   - In `crates/odm-plugin/src/backend.rs`:
     - Implement `PluginBackend` struct.
     - Implement `Backend` trait:
       - `fn kind(&self) -> BackendKind { BackendKind::Plugin }`
       - `async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError>`:
         Invokes plugin resolver, checks resolved URL via HTTP HEAD/GET, returns `ProbeResult`.
       - `async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError>`:
         Runs resolver (or reads resolved download from `ctx`), streams HTTP download using `reqwest` transfer logic, reports progress, and honors speed limiter and control token.
   - In `crates/odm-plugin/src/lib.rs`: export `PluginRegistry`, `PluginBackend`, `PluginManifest`, `PluginError`.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-plugin --test plugin_backend
   ```
   *Expected:* All backend tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-plugin/Cargo.toml Cargo.lock crates/odm-plugin/src/ crates/odm-plugin/tests/plugin_backend.rs
   git commit -m "feat(plugin): implement plugin registry and plugin backend"
   ```

---

### Task 5: Site Grabber and Plugin API Types (`odm-api`) [Completed]

- **Commit Message:** `feat(api): add grabber and plugin request and response dtos`
- **Goal:** Define API models for `POST /api/v1/grabber` and `GET /api/v1/plugins`.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-api/tests/m8_api_types.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `GrabberRequest` and `GrabberResponse` JSON roundtrip.
   - Test 2: `GrabbedFileDto` serialization.
   - Test 3: `PluginDto` and `PluginListResponse` JSON roundtrip.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-api --test m8_api_types
   ```
   *Expected:* Compilation error.
3. **Implement Code:**
   - Create `crates/odm-api/src/grabber.rs`:
     - `GrabberRequest`: `url: String`, `depth: Option<u32>`, `extensions: Option<Vec<String>>`, `pattern: Option<String>`, `respect_robots: Option<bool>`, `max_pages: Option<usize>`.
     - `GrabbedFileDto`: `url: String`, `filename: String`, `found_on: String`, `depth: u32`.
     - `GrabberResponse`: `files: Vec<GrabbedFileDto>`, `pages_crawled: usize`.
   - Create `crates/odm-api/src/plugin.rs`:
     - `PluginDto`: `name: String`, `version: String`, `description: Option<String>`, `role: String`, `url_patterns: Vec<String>`, `command: Vec<String>`.
     - `PluginListResponse`: `plugins: Vec<PluginDto>`.
   - In `crates/odm-api/src/lib.rs`: export `pub mod grabber;` and `pub mod plugin;`.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-api --test m8_api_types
   ```
   *Expected:* All API types tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-api/src/ crates/odm-api/tests/m8_api_types.rs
   git commit -m "feat(api): add grabber and plugin request and response dtos"
   ```

---

### Task 6: Daemon Plugin Wiring and Backend Selection (`odm-daemon`) [Completed]

- **Commit Message:** `feat(daemon): wire plugin registry, list endpoint and plugin backend selection`
- **Goal:** Integrate `PluginRegistry` and `PluginBackend` into the daemon, implement `GET /api/v1/plugins`, and select `BackendKind::Plugin` when a source URL matches an installed plugin pattern.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-daemon/tests/plugins_api.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `GET /api/v1/plugins` without auth returns 401 Unauthorized.
   - Test 2: `GET /api/v1/plugins` with auth returns list of discovered plugins.
   - Test 3: `POST /api/v1/probe` with a URL matching an installed plugin selects `BackendKind::Plugin`.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-daemon --test plugins_api
   ```
   *Expected:* Compilation error.
3. **Implement Code:**
   - Add `odm-plugin = { path = "../odm-plugin" }` to `crates/odm-daemon/Cargo.toml`.
   - In `crates/odm-daemon/src/state.rs`:
     - Hold `Arc<tokio::sync::RwLock<PluginRegistry>>` in `AppState`.
   - In `crates/odm-daemon/src/server.rs`:
     - Discover plugins from `<data_dir>/plugins`.
     - Register `PluginBackend` with the `Engine`.
     - Add route `GET /api/v1/plugins` to router with auth.
   - In `crates/odm-daemon/src/routes.rs`:
     - Implement `get_plugins_handler`: queries `PluginRegistry::list()`, returns `PluginListResponse`.
     - Update backend selection in `probe_handler` and `add_download_handler`: if source URL matches an installed plugin, pick `BackendKind::Plugin`.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-daemon --test plugins_api
   ```
   *Expected:* All plugin API tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-daemon/Cargo.toml Cargo.lock crates/odm-daemon/src/ crates/odm-daemon/tests/plugins_api.rs
   git commit -m "feat(daemon): wire plugin registry, list endpoint and plugin backend selection"
   ```

---

### Task 7: Daemon Site Grabber Endpoint (`odm-daemon`) [Completed]

- **Commit Message:** `feat(daemon): implement post grabber api endpoint`
- **Goal:** Implement `POST /api/v1/grabber` in `odm-daemon` executing `SiteGrabber` on request.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-daemon/tests/grabber_api.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `POST /api/v1/grabber` without auth returns 401 Unauthorized.
   - Test 2: `POST /api/v1/grabber` with invalid URL returns 400 Bad Request.
   - Test 3: `POST /api/v1/grabber` with valid start URL on local test server returns `GrabberResponse` with list of files and pages crawled.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-daemon --test grabber_api
   ```
   *Expected:* Compilation error (grabber endpoint not registered).
3. **Implement Code:**
   - In `crates/odm-daemon/src/server.rs`:
     - Register `POST /api/v1/grabber` with auth middleware.
   - In `crates/odm-daemon/src/routes.rs`:
     - Implement `grabber_handler(State, Json<GrabberRequest>) -> Result<Json<GrabberResponse>, AppError>`:
       - Validates start URL.
       - Builds `GrabberConfig` from request.
       - Runs `SiteGrabber::crawl`.
       - Maps results to `GrabberResponse`.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-daemon --test grabber_api
   ```
   *Expected:* All grabber API tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-daemon/src/ crates/odm-daemon/tests/grabber_api.rs
   git commit -m "feat(daemon): implement post grabber api endpoint"
   ```

---

### Task 8: CLI Commands for Grabber and Plugins (`odm-cli`) [Completed]

- **Commit Message:** `feat(cli): add odm grab and odm plugin list commands`
- **Goal:** Add `odm grab <url>` and `odm plugin list` to the `odm` CLI.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-cli/tests/m8_cli.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `odm grab <url> --depth 1` calls grabber API and prints found files.
   - Test 2: `odm grab <url> --add` calls grabber API and enqueues found files to downloads.
   - Test 3: `odm plugin list` prints discovered plugins.
   - Test 4: `odm plugin list --json` outputs valid JSON.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-cli --test m8_cli
   ```
   *Expected:* Compilation error (`Grab` or `Plugin` CLI subcommands not found).
3. **Implement Code:**
   - In `crates/odm-cli/src/main.rs`:
     - Add `Grab` command to `Commands` enum:
       - `url: String`
       - `--depth: Option<u32>`
       - `--ext: Vec<String>`
       - `--pattern: Option<String>`
       - `--no-robots: bool`
       - `--max-pages: Option<usize>`
       - `--add: bool`
     - Add `Plugin` command to `Commands` enum with subcommand `List`.
   - In `crates/odm-cli/src/client.rs`:
     - Add `grab(&self, req: &GrabberRequest) -> Result<GrabberResponse, ...>`
     - Add `list_plugins(&self) -> Result<PluginListResponse, ...>`
   - Implement handlers in `crates/odm-cli/src/commands/`:
     - `grab`: calls grabber API, displays tabular results, and if `--add` is passed, enqueues each found file via `add_download`.
     - `plugin_list`: prints plugin table (name, version, role, patterns) and security notice.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-cli --test m8_cli
   ```
   *Expected:* All CLI tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-cli/src/ crates/odm-cli/tests/m8_cli.rs
   git commit -m "feat(cli): add odm grab and odm plugin list commands"
   ```

---

### Task 9: GUI Site Grabber and Plugins Screen with Security Notice (`odm-gui`) [Completed]

- **Commit Message:** `feat(gui): implement grabber dialog and plugin list view with security notice`
- **Goal:** Implement the GUI Grabber dialog (start URL, depth, extensions, results table, select and batch add) and Plugin list screen with mandatory security notice.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-gui/tests/m8_gui.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `GrabberViewState` lifecycle: open dialog, start grab action dispatched, results loaded, file checkboxes toggled, batch add payload generated.
   - Test 2: `PluginsViewState` lifecycle: open dialog, plugins loaded, displays plugins and security notice text.
2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-gui --test m8_gui
   ```
   *Expected:* Compilation error.
3. **Implement Code:**
   - In `crates/odm-gui/src/state.rs`:
     - Define `GrabberViewState` and `PluginsViewState`.
   - In `crates/odm-gui/src/messages.rs`:
     - Add `UserAction::StartGrab(GrabberRequest)`, `UserAction::AddGrabbedFiles(Vec<String>)`, `UserAction::RefreshPlugins`.
     - Add `DaemonMessage::GrabCompleted(GrabberResponse)`, `DaemonMessage::PluginsLoaded(PluginListResponse)`.
   - Create `crates/odm-gui/src/ui/grabber_dialog.rs`:
     - URL input, depth slider, extensions filter, pattern filter, respect robots checkbox.
     - "Start Grab" button.
     - Table of found files with checkboxes, "Select All", "Deselect All", and "Add Selected (N)" button.
   - Create `crates/odm-gui/src/ui/plugins_dialog.rs`:
     - Prominent security banner:
       `⚠️ Security Notice: Plugins are external programs running with your full user permissions. There is no sandbox. Only install plugins from sources you trust.`
     - Table of installed plugins (name, version, role, patterns, command).
     - Plugin folder location display with "Open Folder" / "Copy Path" button.
   - In `crates/odm-gui/src/ui/toolbar.rs`:
     - Add `[🌐 Grabber]` and `[🔌 Plugins]` action buttons.
   - In `crates/odm-gui/src/app.rs`:
     - Render grabber and plugins dialogs when active.
4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-gui --test m8_gui
   ```
   *Expected:* All GUI view state tests pass.
5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
6. **Commit:**
   ```powershell
   git add crates/odm-gui/src/ crates/odm-gui/tests/m8_gui.rs
   git commit -m "feat(gui): implement grabber dialog and plugin list view with security notice"
   ```

---

### Task 10: Manual Checklist, Documentation and Milestone 8 Closure [Completed]

- **Commit Message:** `docs: record M8 plugins and site grabber`
- **Goal:** Document Milestone 8 features in `CHANGELOG.md` and provide a comprehensive manual verification checklist in `docs/manual-checklists/m8-plugins-grabber.md`.

#### Steps:
1. **Create Manual Checklist:**
   Create `docs/manual-checklists/m8-plugins-grabber.md` with detailed test scenarios:
   - Plugin manifest discovery and loading from `<data_dir>/plugins`.
   - Resolver plugin execution and JSON stdin/stdout protocol.
   - Plugin 30-second timeout enforcement and process termination.
   - Crash resilience (non-zero exit code handled gracefully without daemon crash).
   - Malformed JSON handling.
   - Site grabber crawl on single page (depth 1) and subpages (depth 2).
   - Same-host restriction enforcement.
   - `robots.txt` obedience and `--no-robots` bypass.
   - Extension filtering and pattern filtering.
   - Batch download addition from grab results.
   - CLI `odm grab` and `odm plugin list`.
   - GUI Grabber dialog and Plugins dialog with security notice.
2. **Update Changelog:**
   Update `CHANGELOG.md` under `## [Unreleased]` describing Milestone 8 additions.
3. **Verify Full Workspace:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```
4. **Commit:**
   ```powershell
   git add CHANGELOG.md docs/manual-checklists/m8-plugins-grabber.md docs/superpowers/plans/2026-11-11-odm-m8-plugins-grabber.md
   git commit -m "docs: record M8 plugins and site grabber"
   ```
