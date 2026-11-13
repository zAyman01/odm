# odm — Open Download Manager

A fast, cross-platform download manager written in Rust. It replaces Internet Download Manager (IDM) with modern capabilities: multi-segment acceleration, BitTorrent, HLS/DASH streaming video, yt-dlp integration, plugins, site crawling, a browser extension, and remote control via Web UI and REST API.

> **Status: v0.1.0 Released.** Ready for daily use across Windows, Linux, and macOS. The full design specification is available in [docs/superpowers/specs/2026-11-11-odm-design.md](docs/superpowers/specs/2026-11-11-odm-design.md).

---

## Features

- **Segmented HTTP & HTTPS Acceleration:** Parallel connections (1–32 segments) with dynamic splitting of remaining parts, live speed throttling, automatic exponential-backoff retries, and crash-resilient resume using SQLite.
- **BitTorrent Engine:** Native torrent client powered by `librqbit`. Downloads magnet links and `.torrent` files with multi-file selection, configurable seed ratios, DHT, peer exchange (PEX), and UPnP port mapping.
- **Streaming Media:** Native HLS (`.m3u8`) and DASH (`.mpd`) video extraction with audio/video muxing via `ffmpeg` and AES-128 decryption.
- **yt-dlp Integration:** Direct format discovery and downloading from any site supported by `yt-dlp` with integrated progress reporting and rate limiting.
- **Checksums & Mirrors:** Cryptographic verification (SHA-256, SHA-1, MD5), multi-mirror load distribution with automatic failover, and Metalink 3 / Meta4 XML parsing.
- **Site Grabber:** Crawl web pages up to a configured depth to extract links and media matching extension or regex filters, with `robots.txt` compliance.
- **Plugin Architecture:** Out-of-process resolver plugins in any language using JSON over stdin/stdout with 30-second isolation and fault protection.
- **Modern Desktop GUI:** Built with `egui`/`eframe` featuring dark/light modes, live segment visualizer, system tray integration, URL clipboard monitoring, and completion notifications.
- **Web UI & REST API:** Embedded single-page Web UI served directly by the daemon (`http://127.0.0.1:6820/`) with WebSocket live events and bearer authentication.
- **Browser Extension:** Manifest V3 for Chromium (Chrome, Edge, Brave) and Firefox with automatic download capture, context menu integration ("Download with odm", "Download all links"), and live video stream sniffing.

*Note: odm does not download DRM-protected streams or live broadcasts.*

---

## Workspace Layout

| Path | Description |
|---|---|
| `crates/odm-core` | Core download engine: state machine, SQLite store, segmented HTTP, limiter, queues, grabber |
| `crates/odm-torrent` | BitTorrent backend adapter over `librqbit` |
| `crates/odm-media` | HLS, DASH, and yt-dlp backends with `ffmpeg` muxing integration |
| `crates/odm-plugin` | Out-of-process plugin host and URL pattern resolver |
| `crates/odm-api` | Shared DTOs, request/response models, and WebSocket event types |
| `crates/odm-daemon` | Headless background server with `axum` HTTP/WebSocket API and embedded Web UI |
| `crates/odm-cli` | Command-line client (`odm`) with human-readable and `--json` modes |
| `crates/odm-gui` | Desktop GUI (`odm-gui`) with system tray and egui interface |
| `extension/` | Browser extension for Chrome and Firefox (Manifest V3) |
| `webui/` | Static Web UI embedded into `odm-daemon` |

---

## Installation

Prebuilt release packages are available on the [GitHub Releases](https://github.com/zAyman01/odm/releases) page.

### Windows (`x86_64`)
1. Download `odm-v0.1.0-x86_64-pc-windows-msvc.zip`.
2. Extract the archive into a folder of your choice (e.g. `C:\Program Files\odm` or `C:\Users\<user>\AppData\Local\Programs\odm`).
3. Double-click `odm-gui.exe` to launch the desktop application (or run `odm.exe` from PowerShell / Command Prompt).

> [!NOTE]
> **Windows Defender SmartScreen Notice:**  
> The v0.1.0 release binaries are open source and currently unsigned. When running on Windows, Defender SmartScreen may display:  
> *"Windows protected your PC: Microsoft Defender SmartScreen prevented an unrecognized app from starting."*  
> To continue, click **More info**, then click **Run anyway**.

### Linux (`x86_64`)
1. Download `odm-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`.
2. Extract the archive:
   ```bash
   tar -xzf odm-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
   cd odm-v0.1.0-x86_64-unknown-linux-gnu
   chmod +x odm odm-daemon odm-gui
   ```
3. Run `./odm-gui` for the desktop client, or `./odm` for the CLI.

### macOS (Apple Silicon `aarch64` & Intel `x86_64`)
1. Download the archive for your architecture:
   - Apple Silicon (M1/M2/M3/M4): `odm-v0.1.0-aarch64-apple-darwin.tar.gz`
   - Intel: `odm-v0.1.0-x86_64-apple-darwin.tar.gz`
2. Extract the archive:
   ```bash
   tar -xzf odm-v0.1.0-*-apple-darwin.tar.gz
   chmod +x odm odm-daemon odm-gui
   ```

> [!NOTE]
> **macOS Gatekeeper Notice:**  
> The v0.1.0 release binaries are not notarized with an Apple Developer ID. If macOS displays:  
> *"odm-gui cannot be opened because Apple cannot check it for malicious software"*,  
> right-click (or Control-click) `odm-gui` in Finder, click **Open**, and confirm in the dialog prompt. Alternatively, approve the app in **System Settings > Privacy & Security**.

---

## Browser Extension Setup

The browser extension integrates odm with Chrome, Firefox, Edge, Brave, and other Chromium browsers.

1. Download `odm-extension-chrome-v0.1.0.zip` (for Chromium browsers) or `odm-extension-firefox-v0.1.0.zip` (for Firefox) from GitHub Releases and extract it to a directory.
2. **In Chrome / Chromium:**
   - Navigate to `chrome://extensions/`.
   - Enable **Developer mode** in the top right.
   - Click **Load unpacked** and select the extracted extension directory.
3. **In Firefox:**
   - Navigate to `about:debugging#/runtime/this-firefox`.
   - Click **Load Temporary Add-on...** and select `manifest.json` from the extracted directory.
4. **Configuration:**
   - Right-click the odm extension icon in your browser toolbar and choose **Options** (or click the Settings gear in the extension popup).
   - Enter your bearer token (found in `<config_dir>/odm/token`).
   - Click **Test Connection** to verify communication with the daemon at `http://127.0.0.1:6820`.

---

## Getting Started

### 1. Launching the GUI
Run `odm-gui`. The GUI automatically checks if `odm-daemon` is running; if not, it spawns the daemon in the background. Closing the window minimizes odm to the system tray.

### 2. Using the Web UI
The daemon embeds a lightweight, responsive Web UI:
1. Open `http://127.0.0.1:6820/` in any desktop or mobile browser.
2. Enter the bearer token from your config folder:
   - **Windows:** `%APPDATA%\odm\config\token`
   - **Linux:** `~/.config/odm/token`
   - **macOS:** `~/Library/Application Support/odm/token`
3. View real-time progress over WebSocket, add URLs, and pause/resume/cancel downloads.

### 3. Command-Line Client (`odm`)

The CLI provides full scriptable control over all features:

```bash
# Add an HTTP download
odm add https://example.com/large-archive.zip

# Add with options: 16 segments, 5 MB/s speed limit, SHA-256 integrity check
odm add https://example.com/file.iso --segments 16 --limit 5242880 --hash sha256:e3b0c442...

# Add multiple mirrors for failover
odm add https://mirror1.com/file.zip --mirrors https://mirror2.com/file.zip,https://mirror3.com/file.zip

# Add a BitTorrent magnet link with custom seed ratio
odm add "magnet:?xt=urn:btih:..." --seed-ratio 1.5

# Add a streaming video with quality selection
odm add https://example.com/playlist.m3u8 --quality 1080p

# Batch download URL expansion
odm add "https://example.com/images/photo_[001-050].jpg"

# Crawl a site up to depth 2 for images and archives
odm grab https://example.com/gallery --depth 2 --ext zip,png,jpg --add

# Inspect daemon state and active jobs
odm daemon status
odm list
odm info <job-id>
odm pause <job-id>
odm resume <job-id>

# Output machine-readable JSON
odm list --json
```

---

## External Tools

- **`ffmpeg`:** Required for muxing DASH video and audio tracks together into `.mp4`, and optional for converting HLS `.ts` streams into `.mp4`. Ensure `ffmpeg` is installed and on your system `PATH`, or set `ffmpeg_path` in odm settings.
- **`yt-dlp`:** Required for downloading videos from sites supported by yt-dlp. odm looks for `yt-dlp` on your system `PATH` or in settings. The desktop GUI also offers a one-click download for `yt-dlp` on first use.

---

## Security Model

- **Authentication:** All daemon REST endpoints (except `GET /api/v1/health`) and WebSocket event streams require HTTP Bearer token authentication. A cryptographically random token is generated on initial run and stored with owner-only read permissions.
- **DNS Rebinding Protection:** The daemon validates incoming `Host` headers against an allow-list (`127.0.0.1`, `localhost`, and the configured remote bind address).
- **CORS:** Cross-Origin Resource Sharing is strictly scoped to the daemon's own origin and the browser extension.
- **Plugin Notice:** Plugins run as external child processes under your local user account without sandboxing. Only install plugins from trusted sources.

---

## Building from Source

Install the Rust toolchain via [rustup](https://rustup.rs). The pinned compiler version (`1.99`) will be configured automatically:

```bash
# Clone the repository
git clone https://github.com/zAyman01/odm.git
cd odm

# Build release binaries for all workspace members
cargo build --workspace --release --locked

# Run the complete test suite
cargo test --workspace --locked

# Package the browser extensions
node scripts/package-extension.mjs --out-dir dist
```

Binaries will be placed in `target/release/`:
- `target/release/odm` (or `odm.exe`)
- `target/release/odm-daemon` (or `odm-daemon.exe`)
- `target/release/odm-gui` (or `odm-gui.exe`)

---

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before opening pull requests.  
For vulnerability disclosures, see [SECURITY.md](SECURITY.md).

---

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
