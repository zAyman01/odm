# odm (Open Download Manager) — Design Spec

Date: 2026-11-11
Status: awaiting review

## 1. Purpose

`odm` is an open source, cross-platform download manager written in Rust. It aims to replace Internet Download Manager (IDM) as a daily driver and to add torrents, yt-dlp support, plugins, mirrors and remote control.

It is the author's first open source project. The repository must be ready for outside contributors from the first release.

### Constraints

- Implementation is done by a weaker coding model running in opencode. The author is new to Rust and cannot review Rust code by eye. Correctness therefore rests on small tasks, tests written before code, and strict CI.
- Everything in this spec ships in v0.1.0. Work is still built in milestones (section 12), each with its own implementation plan, and each gated before the next starts.
- License: `MIT OR Apache-2.0`.
- Platforms: Windows, Linux, macOS.
- All dependencies and external tools are free and open source.

### Success criteria

- A user can install `odm`, capture a download from Chrome or Firefox, and have it download faster than the browser through segmented transfer, with pause and resume surviving a restart.
- Magnet links, HLS/DASH streams and yt-dlp-supported sites download through the same interface.
- CI is green on all three platforms, and the repository has the standard contributor documents.

### Non-goals for v0.1

- FTP and SFTP.
- DRM-protected streams (never supported).
- Live streams.
- Built-in TLS for the daemon.
- Plugin sandboxing, plugin marketplace, or automatic plugin installation.
- Automatic updates of `odm` itself.
- Translations (English only).
- Mobile apps, accounts, cloud sync.

## 2. Architecture

`odm` is a headless daemon that owns all download work, plus clients that talk to it over a local HTTP API. Clients never touch the database or the engine directly.

### Cargo workspace

| Crate | Kind | Responsibility |
|---|---|---|
| `odm-core` | lib | Job model, state machine, queue, scheduler, speed limiter, SQLite persistence, checksums, event bus, HTTP backend, site grabber, batch URL expansion |
| `odm-torrent` | lib | Torrent backend, an adapter over `librqbit` |
| `odm-media` | lib | HLS/DASH backend and yt-dlp backend |
| `odm-plugin` | lib | External-process plugin host and plugin backend |
| `odm-api` | lib | Request, response and event types shared by daemon and clients (`serde`) |
| `odm-daemon` | bin | `axum` REST and WebSocket server, auth, serves the web UI |
| `odm-cli` | bin (named `odm`) | Command-line client |
| `odm-gui` | bin | egui desktop client with tray icon |

The crate name `odm` is taken on crates.io by an unrelated project, so the CLI is published as `odm-cli` with a binary named `odm`.

Outside Cargo:

- `extension/` — browser extension, Manifest V3, plain JavaScript, no build step, one codebase with separate manifests for Chrome and Firefox.
- `webui/` — one static page of HTML, CSS and vanilla JavaScript, no build step, embedded into the daemon binary with `rust-embed`.

### The Backend trait

`odm-core` defines one trait that every download method implements. The queue, scheduler, limiter, persistence and API know only this trait. Adding a protocol means adding a backend and nothing else.

```rust
#[async_trait]
pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    /// Inspect the source without downloading: name, size, selectable items.
    async fn probe(&self, source: &Source, opts: &JobOptions) -> Result<ProbeResult, BackendError>;
    /// Run or resume the job until it finishes or the control token fires.
    async fn run(&self, ctx: JobContext) -> Result<RunOutcome, BackendError>;
}

pub enum RunOutcome { Completed, Paused, Cancelled }
```

`JobContext` carries the job record, a control token that signals pause or cancel, a progress sink, the shared speed limiter, and a handle for saving backend-specific resume state. Pause and resume are not separate methods: pausing fires the token and `run` returns `Paused` after saving state; resuming calls `run` again and the backend continues from its saved state.

### Backend selection

When a download is added, the core picks a backend in this order. A client may override the choice with an explicit `backend` field.

1. `magnet:` scheme, a `.torrent` file path, or a URL whose response type is `application/x-bittorrent`: torrent.
2. A plugin whose URL pattern matches: plugin.
3. URL path ending in `.m3u8` or `.mpd`, or an HLS/DASH content type from the HTTP probe: HLS/DASH.
4. HTTP probe returns `text/html`, yt-dlp is available, and yt-dlp reports formats for the URL: yt-dlp.
5. Otherwise: HTTP.

### Processes

- The daemon is headless and binds `127.0.0.1:6820` by default. If the port is in use it exits with a clear message.
- The GUI checks `/api/v1/health` at start and spawns the daemon if it is not running. Closing the GUI window hides it to the tray. Quitting the GUI does not stop the daemon.
- A setting enables starting the daemon at login, so the extension works without the GUI open.
- Data locations come from the `directories` crate: config folder for settings and the auth token, data folder for the SQLite database, torrent resume data and plugins. The default download folder is the OS Downloads folder.

### Data flow

1. A client sends `POST /api/v1/downloads`.
2. The daemon passes it to the core, which selects a backend, creates a job record and places it in a queue.
3. The queue starts the job when a slot is free. The backend runs and reports progress.
4. Events go onto a broadcast channel. The daemon forwards them over WebSocket to every connected client.

## 3. Core engine (`odm-core`)

### Job states

`Queued`, `Probing`, `Downloading`, `Paused`, `Verifying`, `Seeding`, `Completed`, `Failed`, `Cancelled`.

Allowed transitions:

| From | To |
|---|---|
| `Queued` | `Probing`, `Paused`, `Cancelled` |
| `Probing` | `Downloading`, `Failed`, `Paused`, `Cancelled` |
| `Downloading` | `Paused`, `Verifying`, `Seeding`, `Completed`, `Failed`, `Cancelled` |
| `Paused` | `Queued`, `Cancelled` |
| `Verifying` | `Completed`, `Failed` |
| `Seeding` | `Completed`, `Paused`, `Cancelled` |
| `Failed` | `Queued` (retry), `Cancelled` |

`Completed` and `Cancelled` are final. All transitions go through one function that rejects anything not in this table. `Seeding` is used only by the torrent backend.

### HTTP backend

- **Probe.** Send `HEAD`. If that fails or lacks size information, send `GET` with `Range: bytes=0-0`. Record total size, whether ranges are supported, `ETag`, `Last-Modified`, and the filename from `Content-Disposition`, falling back to the last URL path segment, then to `download`.
- **Segmented download.** When ranges are supported and the size is known, preallocate `<name>.part` and split the file into segments. Default 8, configurable from 1 to 32. Each segment is a `tokio` task that writes at its own offset.
- **Dynamic segmentation.** When a segment finishes, the largest remaining segment is split in half and the free connection takes the second half. A segment with less than 1 MiB remaining is not split.
- **Single stream.** When ranges are not supported or the size is unknown, download in one stream. Such a job cannot resume; pausing it restarts from zero, and clients show this.
- **Per-job options.** Extra headers, cookies, referer, user agent, basic auth credentials, HTTP or SOCKS5 proxy.
- **Finish.** When all segments complete, rename `<name>.part` to `<name>`.

### Resume

Segment offsets are written to SQLite every 1 second while downloading and immediately on pause. On resume, requests carry `If-Range` with the saved `ETag` (or `Last-Modified` if there is no `ETag`). If the server answers `200` instead of `206`, the file changed: discard progress and restart from zero.

After a crash or power loss, jobs that were `Probing`, `Downloading` or `Verifying` load as `Paused`; `Seeding` jobs resume seeding.

### Failure handling

- Each segment retries on network errors and 5xx responses with exponential backoff: 1, 2, 4, 8, 16 seconds, 5 attempts by default.
- 4xx responses other than 408 and 429 fail immediately.
- When a segment exhausts its retries the job becomes `Failed` with a message and a `retryable` flag. Completed bytes are kept so a manual retry continues from them.
- Disk full and permission errors fail the job with a specific message and are not retried.
- Library code uses typed errors (`thiserror`). No `unwrap` or `expect` outside tests.

### Speed limiter

A token bucket, one global and one optional per job. All segments of a job draw from both. A limit of 0 means unlimited. Backends that do not move bytes themselves apply the limit their own way (section 4).

### Queues and scheduler

- Jobs belong to a named queue. A default queue always exists.
- Each queue has a maximum number of concurrent jobs (default 3) and an ordered job list. Order can be changed.
- A queue may have a schedule: start time, optional stop time, and weekdays. The daemon checks schedules once a minute, starts the queue at the start time and pauses its jobs at the stop time.

### Integrity and mirrors

- A job may carry an expected hash (SHA-256, SHA-1 or MD5). After download the job enters `Verifying`, the file is hashed in a streaming pass, and a mismatch fails the job.
- A job may carry several URLs for the same file. Segments are spread across mirrors. A mirror that fails its retries is dropped and its segments move to the others.
- Metalink files (`.metalink`, `.meta4`) are parsed into a job with mirrors and hashes.

### Files and categories

- If the target filename exists, ` (1)`, ` (2)` and so on are appended before the extension.
- Categories map file extensions to folders. Defaults: Compressed, Documents, Music, Video, Programs, Other. Users can edit them.

### Site grabber

Given a start URL, crawl pages on the same host up to a depth limit, collecting links that match extension or pattern filters. `robots.txt` is respected by default. HTML is parsed with `scraper`. The result is a list of files; the user selects which to add, and they are added as a batch.

### Batch URLs

A pattern such as `https://example.com/file[001-100].jpg` expands to a list of jobs. Zero padding follows the pattern.

### Persistence

SQLite through `rusqlite` with the bundled feature. Tables: `jobs`, `segments`, `queues`, `categories`, `settings`. Schema changes go through numbered migrations applied at startup. Backend-specific resume state is stored as a JSON column on `jobs`.

### Events

A `tokio` broadcast channel carries: job added, job state changed, job progress, job removed, queue changed. Progress is limited to 4 events per second per job.

## 4. Other backends

### Torrent (`odm-torrent`)

- One `librqbit` session lives inside the daemon. It accepts magnet links and `.torrent` files.
- Probe returns the file list so the user can select files before starting. For magnet links probe waits for metadata.
- DHT, peer exchange and UPnP port mapping come from `librqbit`.
- After download the job enters `Seeding`. It seeds to ratio 1.0 by default, then becomes `Completed`. The ratio is configurable; 0 skips seeding.
- `librqbit` stores its own resume data in the data folder. `odm` stores only the mapping from job to torrent.
- Speed limits are applied through `librqbit`'s rate limit options.

### HLS and DASH (`odm-media`)

- **HLS.** `m3u8-rs` parses the playlist. Probe lists the available qualities. Segments are fetched through the HTTP backend's transfer code, so the limiter and retry rules apply. AES-128 encrypted segments are decrypted.
- **DASH.** The `dash-mpd` crate parses the manifest. Audio and video tracks are fetched separately.
- **Muxing.** `ffmpeg` joins the result into `.mp4`. It is found from the settings path, then `PATH`. Without `ffmpeg`, HLS keeps the joined `.ts` file and DASH jobs fail with a message explaining that `ffmpeg` is required. `ffmpeg` is not bundled.
- DRM-protected streams fail at probe with a clear message.

### yt-dlp (`odm-media`)

- The binary is found from the settings path, then `PATH`. If missing, clients offer to download it from the official GitHub release into the data folder, after the user confirms.
- Probe runs `yt-dlp -J <url>` and returns the formats.
- yt-dlp performs the download itself. `odm` runs it as a child process with a fixed progress template and parses the output into progress events. Site URLs expire and are throttled, and yt-dlp handles that.
- Pause stops the process. Resume reruns it with `--continue`. The speed limit is passed with `--limit-rate`.

### Plugins (`odm-plugin`)

- A plugin is a folder in the plugins directory containing `plugin.toml` (name, version, URL patterns, command) and an executable written in any language.
- v0.1 supports one plugin role, the resolver. `odm` starts the plugin, writes one JSON request containing the URL to its stdin, and reads one JSON response: a list of direct downloads, each with URL, headers and filename. The HTTP backend then downloads them.
- A plugin has 30 seconds to answer. A timeout, crash, or invalid JSON fails the job and does not affect the daemon.
- **Security.** Plugins are ordinary programs running with the user's full permissions. There is no sandbox. `odm` never downloads or installs plugins; installation is a manual copy into the plugins folder. The documentation and the GUI plugin screen state that a plugin can do anything the user can do on the machine.

## 5. Daemon API (`odm-daemon`)

REST with JSON bodies under `/api/v1`.

| Method and path | Purpose |
|---|---|
| `GET /health` | Liveness and version. The only route without auth. |
| `POST /probe` | Inspect a URL: backend, filename, size, qualities or torrent files |
| `POST /downloads` | Add one or more downloads |
| `GET /downloads` | List jobs, filterable by state, queue, category |
| `GET /downloads/{id}` | One job with segment detail |
| `POST /downloads/{id}/pause`, `/resume`, `/cancel`, `/retry` | Control a job |
| `DELETE /downloads/{id}` | Remove a job; optional flag deletes the file |
| `GET, POST, PATCH, DELETE /queues` | Manage queues and schedules |
| `GET, PUT /categories` | Manage categories |
| `GET, PATCH /settings` | Read and change settings |
| `POST /grabber` | Run a site grab and return the found files |
| `GET /plugins` | List installed plugins |
| `GET /events` | WebSocket stream of events |

Errors use one shape: `{"error": {"code": "<machine_code>", "message": "<text>"}}` with a matching HTTP status.

### Security

- A random token is generated on first run and stored in the config folder in a file readable only by the owner. Every route except `/health` requires `Authorization: Bearer <token>`. The WebSocket takes the token in the first message.
- The CLI and GUI read the token file. For the extension and remote web UI the user pastes the token.
- The `Host` header is checked against an allow-list (`127.0.0.1`, `localhost`, plus the configured remote address). This blocks DNS rebinding.
- CORS allows only the extension's origin and the daemon's own origin.
- Remote access is off by default. Enabling it binds a non-loopback address. Traffic is plain HTTP, so the token is unencrypted on the network; the setting and the documentation say to use it only on a trusted LAN or behind a VPN or TLS reverse proxy.

## 6. Clients

### CLI (`odm`)

Built with `clap`. Commands: `odm add <url>...`, `odm list`, `odm info <id>`, `odm pause|resume|cancel|retry|remove <id>`, `odm queue ...`, `odm daemon start|stop|status`. Every command accepts `--json`. `odm add` accepts options for folder, filename, segments, speed limit, headers, hash, mirrors and queue.

### GUI (`odm-gui`)

Built with `eframe`/`egui`.

```
[Add] [Resume] [Pause] [Remove] [Grabber] [Settings]
+-----------+-------------------------------------------+
| All       | Name     Size  Progress  Speed  ETA Status|
| Active    | file.zip 1.2G  ####--    8 MB/s 1m  ...   |
| Completed |                                           |
| Torrents  |                                           |
| Categories+-------------------------------------------+
| Queues    | Detail: segment map / torrent files, peers|
+-----------+-------------------------------------------+
```

- **Add dialog.** Probes first, then shows filename, size, target folder, and quality or file selection where the backend offers it. Accepts batch patterns and multiple URLs.
- **Other screens.** Settings, queue and schedule editor, grabber, plugin list with the security notice.
- **Desktop integration.** Tray icon (`tray-icon`), completion notifications (`notify-rust`), clipboard monitoring (`arboard`) that offers to add a copied download URL; clipboard monitoring can be turned off.
- **Theme.** Dark and light.
- **Structure.** All network calls run on a background thread with a `tokio` runtime and reach the UI through channels; the UI thread never blocks. View state and its update logic live in plain structs separate from drawing code, so they can be unit tested.

### Web UI

One page served by the daemon: job list with live progress over WebSocket, add URL, pause, resume, remove. It asks for the token once and keeps it in browser storage. No settings, no grabber.

### Browser extension

- **Capture.** Intercepts downloads started in the browser, cancels the browser's copy, and sends URL, cookies, referer and user agent to the daemon. Filters: minimum file size, extension list, per-site disable.
- **Context menu.** "Download with odm" on links, and "Download all links" on pages.
- **Media detection.** Observes requests for `.m3u8`, `.mpd` and video content types. The toolbar badge shows the count and the popup lists them for sending to `odm`.
- **Options.** Token, port, filters.
- **Fallback.** If the daemon is unreachable, the browser download proceeds normally.
- **Privacy.** Cookies and URLs are sent only to the local daemon.

## 7. Settings

Stored in the `settings` table and exposed through the API: default download folder, default segments, global speed limit, default retry count, proxy, default seed ratio, torrent listen port, paths to `ffmpeg` and yt-dlp, clipboard monitoring, start daemon at login, remote access (bind address), theme.

## 8. Testing

Tests never use the real internet.

- **HTTP backend.** A local test server serves files with ranges, without ranges, slowly, dropping mid-stream, and with a changing `ETag`. A test passes when the downloaded file's hash equals the source's.
- **State machine, limiter, queue, scheduler, batch expansion, filename conflicts.** Unit tests. Time-dependent code takes an injectable clock.
- **HLS/DASH.** A local server serves a generated playlist or manifest and segments.
- **Torrent.** Two `librqbit` sessions on localhost; one seeds a small generated file.
- **yt-dlp and plugins.** A fake executable built as a test helper emits canned output, including malformed output and hangs.
- **API.** Routes are tested in-process, including auth, `Host` check and error shapes.
- **GUI.** View-state logic is unit tested. Drawing is covered by a manual checklist in the repository.
- **Extension.** Filter logic is unit tested with `node --test`. Capture is covered by a manual checklist.

## 9. CI and release

GitHub Actions on Windows, Linux and macOS:

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo deny check` for licenses and security advisories

Pushing a version tag builds release binaries for the three platforms and attaches them to a GitHub Release. Binaries are unsigned in v0.1; the README explains the resulting OS warnings.

## 10. Guard rails for the implementing model

- `#![forbid(unsafe_code)]` in every crate.
- Clippy lints deny `unwrap` and `expect` outside tests.
- `rust-toolchain.toml` pins the toolchain. `Cargo.lock` is committed.
- Each implementation plan pins dependency versions, verified against current documentation when the plan is written.
- `AGENTS.md` instructs the model to run format, clippy and tests before finishing a task; never to change the `Backend` trait or `odm-api` types unless the task says so; and to make one commit per task.
- Plan tasks touch one file where possible and give the exact path, exact signatures, the test code first, the command to run and the expected output.
- At the end of each milestone the diff is reviewed by the planning model before the next milestone starts.

## 11. Repository files

`README.md`, `LICENSE-MIT`, `LICENSE-APACHE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `CHANGELOG.md`, `AGENTS.md`, `deny.toml`, issue templates and a pull request template.

## 12. Milestones

Each milestone gets its own implementation plan. The gate for each is green CI plus a manual check of the listed result.

| # | Milestone | Result to check |
|---|---|---|
| M0 | Workspace skeleton, CI, repository files | Empty crates build; CI green on three platforms |
| M1 | Core: job model, state machine, SQLite, single-stream HTTP | A test downloads a file from the local server |
| M2 | Core: segments, dynamic segmentation, resume, retry, limiter, queue | Segmented download resumes after interruption |
| M3 | Daemon API and CLI | `odm add <url>` downloads a real file |
| M4 | GUI: list, add dialog, controls, tray, notifications | Downloads managed from the window |
| M5 | Checksums, mirrors, Metalink, categories, scheduler, batch URLs | Each feature usable from CLI and GUI |
| M6 | Torrent backend | A magnet link downloads and seeds |
| M7 | HLS/DASH and yt-dlp backends | A stream and a yt-dlp URL download |
| M8 | Plugins and site grabber | A sample plugin resolves a URL; a grab lists files |
| M9 | Web UI and browser extension | Browser download is captured; phone controls a job |
| M10 | Packaging, documentation, release | v0.1.0 binaries published |
