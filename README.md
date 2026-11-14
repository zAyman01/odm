# odm — Open Download Manager

A fast, cross-platform download manager written in Rust. It replaces Internet Download Manager (IDM) with modern capabilities: multi-segment acceleration, BitTorrent, HLS/DASH streaming video, yt-dlp integration, plugins, site crawling, a browser extension, and remote control via Web UI and REST API.

> **Status: v0.1.0 Released.** Ready for daily use across Windows, Linux, and macOS. The full design specification is available in [docs/superpowers/specs/2026-11-11-odm-design.md](docs/superpowers/specs/2026-11-11-odm-design.md).

---

## Features

- **Segmented HTTP & HTTPS Acceleration:** Parallel connections (1–32 segments) with dynamic splitting of remaining parts, live speed throttling, automatic exponential-backoff retries, and crash-resilient resume using SQLite.
- **BitTorrent Engine:** Native torrent client powered by `librqbit`. Downloads magnet links and `.torrent` files with multi-file selection, configurable seed ratios, DHT, and peer exchange (PEX). Torrent progress is not restored after a daemon restart, and UPnP port mapping is not supported.
- **Streaming Media:** Native HLS (`.m3u8`) and DASH (`.mpd`) video extraction with audio/video muxing via `ffmpeg` and AES-128 decryption.
- **yt-dlp Integration:** Direct format discovery and downloading from any site supported by `yt-dlp` with integrated progress reporting and rate limiting.
- **Checksums & Mirrors:** Cryptographic verification (SHA-256, SHA-1, MD5), multi-mirror load distribution with automatic failover, and Metalink 3 / Meta4 XML parsing.
- **Site Grabber:** Crawl web pages up to a configured depth to extract links and media matching extension or wildcard (`*`, `?`) filters, with `robots.txt` compliance.
- **Plugin Architecture:** Out-of-process resolver plugins in any language using JSON over stdin/stdout with 30-second isolation and fault protection.
- **Modern Desktop GUI:** Built with `egui`/`eframe` featuring dark/light modes, a live segment visualizer, URL clipboard monitoring, and completion notifications. The GUI connects to a running daemon; it does not start one and has no system tray icon.
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
| `crates/odm-gui` | Desktop GUI (`odm-gui`) built with egui |
| `extension/` | Browser extension for Chrome and Firefox (Manifest V3) |
| `webui/` | Static Web UI embedded into `odm-daemon` |

---

## Installation

Download an installer from the [GitHub Releases](https://github.com/zAyman01/odm/releases) page. Each installer puts odm, the daemon, the GUI and the browser extension files on your computer. Chrome, Edge, Brave and Firefox only let you turn on an extension that is not in their store by hand, so the last step is yours: follow the short steps in `extension/INSTALL.txt` (the installer opens it for you).

### Windows (`x86_64`)
1. Download `odm-v0.1.0-x86_64-pc-windows-msvc-setup.exe` and run it. It installs for your user only, so it does not ask for administrator rights.
2. Tick the options you want: add `odm` to PATH, start the download service when you sign in, desktop shortcut.
3. Finish with **Start the odm download service now** and **Open odm** ticked. Then add the browser extension (see below).

Uninstall from **Settings > Apps**. The portable `odm-v0.1.0-x86_64-pc-windows-msvc.zip` is also available.

> [!NOTE]
> **Windows Defender SmartScreen Notice:**
> The v0.1.0 release binaries are open source and currently unsigned. When running on Windows, Defender SmartScreen may display:
> *"Windows protected your PC: Microsoft Defender SmartScreen prevented an unrecognized app from starting."*
> To continue, click **More info**, then click **Run anyway**.

### Linux (Debian, Ubuntu and derivatives, `x86_64`)
1. Download `odm-v0.1.0-amd64.deb`.
2. Install it: `sudo apt install ./odm-v0.1.0-amd64.deb`
3. Start the download service for your user: `systemctl --user enable --now odm-daemon`
4. Open **odm** from your applications menu, or run `odm-gui`. The extension files are in `/usr/share/odm/extension`.

On other distributions use `odm-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` (the files sit at the archive root):
```bash
mkdir odm && tar -xzf odm-v0.1.0-x86_64-unknown-linux-gnu.tar.gz -C odm
cd odm
chmod +x odm odm-daemon odm-gui
```
Then follow [How to Run odm](#how-to-run-odm).

### macOS (Apple Silicon `aarch64` & Intel `x86_64`)
1. Download the disk image for your Mac:
   - Apple Silicon (M1/M2/M3/M4): `odm-v0.1.0-aarch64-apple-darwin.dmg`
   - Intel: `odm-v0.1.0-x86_64-apple-darwin.dmg`
2. Open it, right-click **Install odm.command**, choose **Open**, and confirm.
3. The script copies odm to `~/Applications/odm`, asks whether to start the download service when you sign in, and opens odm. The extension files are in `~/Applications/odm/extension`.

The `.tar.gz` archives are also available; extract one, then follow [How to Run odm](#how-to-run-odm).

> [!NOTE]
> **macOS Gatekeeper Notice:**
> The v0.1.0 release binaries are not notarized with an Apple Developer ID. If macOS displays:
> *"Install odm.command cannot be opened because Apple cannot check it for malicious software"*,
> right-click (or Control-click) it in Finder, click **Open**, and confirm in the dialog prompt. Alternatively, approve it in **System Settings > Privacy & Security**.

---

## How to Run odm

odm has three programs. `odm-daemon` does the downloading; `odm-gui` and `odm` (the CLI) are clients that talk to it. **Always start the daemon first.**

### 1. Start the daemon

Run it from the folder where you extracted the release (or `target/release/` after a source build):

```bash
# Linux / macOS
./odm-daemon

# Windows (PowerShell)
.\odm-daemon.exe
```

The daemon listens on `http://127.0.0.1:6820` and keeps running until you close it (Ctrl+C stops it cleanly and pauses active jobs). On the first run it creates the config folder and a token file (see [Using the Web UI](#2-using-the-web-ui) for the location). Useful options:

| Option | Meaning |
|---|---|
| `--bind 127.0.0.1:7000` | Listen on another address or port |
| `--config-dir <path>` | Keep the config and token in another folder |
| `--data-dir <path>` | Keep the database and torrent files in another folder |

### 2. Start a client

With the daemon running, use any of these:

- **Desktop GUI:** run `./odm-gui` (`odm-gui.exe` on Windows). It finds the token on its own and shows "Daemon Connected" in the toolbar. "Daemon Offline" means the daemon is not running.
- **CLI:** run `./odm add <url>`, then `./odm list`. See [Command-Line Client](#3-command-line-client-odm) below.
- **Web UI:** open `http://127.0.0.1:6820/` in a browser and paste the token.
- **Browser extension:** see [Browser Extension Setup](#browser-extension-setup).

If you changed `--bind` or the config folder, tell the clients: set `ODM_SERVER` (for example `http://127.0.0.1:7000`) and `ODM_TOKEN` (the contents of the token file), or pass `--server` and `--token` to `odm`.

### 3. Run from source instead

```bash
# Terminal 1: the daemon
cargo run --release -p odm-daemon

# Terminal 2: the GUI, or the CLI
cargo run --release -p odm-gui
cargo run --release -p odm-cli -- add https://example.com/file.zip
```

---

## Browser Extension Setup

The browser extension integrates odm with Chrome, Firefox, Edge, Brave, and other Chromium browsers.

If you used an installer, the unpacked extension is already on your computer in the `extension` folder of the install location (`chrome` and `firefox` inside it); skip to step 2.

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
Start `odm-daemon` first, then run `odm-gui`. The GUI does not start the daemon; it connects to one on `http://127.0.0.1:6820` and shows "Daemon Offline" in the toolbar until the daemon is running. Closing the window exits the GUI; the daemon keeps running.

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
odm add https://example.com/file.iso --segments 16 --limit 5242880 --checksum sha256:e3b0c442...

# Add multiple mirrors for failover
odm add https://mirror1.com/file.zip --mirror https://mirror2.com/file.zip --mirror https://mirror3.com/file.zip

# Add a BitTorrent magnet link with custom seed ratio
odm add "magnet:?xt=urn:btih:..." --seed-ratio 1.5

# Add a streaming video with quality selection
odm add https://example.com/playlist.m3u8 --quality 1080p

# Batch download URL expansion
odm add "https://example.com/images/photo_[001-050].jpg"

# Crawl a site up to depth 2 for images and archives
odm grab https://example.com/gallery --depth 2 --ext "zip png jpg" --add

# Check that the daemon is reachable, then inspect jobs
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
- **`yt-dlp`:** Required for downloading videos from sites supported by yt-dlp. odm looks for `yt-dlp` on your system `PATH` or in settings. Install it yourself; odm does not download it.

---

## Security Model

- **Authentication:** All daemon REST endpoints (except `GET /api/v1/health`) and WebSocket event streams require HTTP Bearer token authentication. A 256-bit token is generated from the operating system's random source on the first run and stored with owner-only read permissions.
- **DNS Rebinding Protection:** The daemon validates incoming `Host` headers against an allow-list (`127.0.0.1`, `localhost`, and the configured remote bind address).
- **CORS and WebSocket origin:** Only `http://127.0.0.1:<port>`, `http://localhost:<port>` and `http://[::1]:<port>` (the daemon's own port) are allowed, plus any `chrome-extension://` or `moz-extension://` origin. Extension ids differ per install, so the extension scheme is allowed as a whole; the bearer token still guards every request. A WebSocket upgrade with any other `Origin` is refused with 403, and a request with no `Origin` (the CLI) is accepted.
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
