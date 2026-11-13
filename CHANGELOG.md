# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- None yet.

## [0.1.0] - 2026-11-13

### Added

#### Core Engine & Transfers
- Core job model, SQLite persistence (`rusqlite` bundled), and finite state machine with strict transition validation.
- Multi-segment HTTP & HTTPS acceleration with dynamic splitting of remaining byte ranges, token-bucket speed limiter, and configurable connection counts (1–32).
- Crash-resilient download resume using saved segment progress and `ETag`/`Last-Modified` validation.
- Per-segment exponential-backoff retry logic with transient network error recovery.
- Priority queue scheduler with concurrency limits and automated queue start/stop schedules.
- Cryptographic integrity verification (SHA-256, SHA-1, MD5) with streaming verification pass in `Verifying` state.
- Multi-mirror downloads with dynamic segment distribution and automatic mirror failover.
- Metalink 3 and Meta4 XML file parser.
- Batch URL pattern expansion supporting numeric (`[001-100]`) and alphabetic ranges with Cartesian products.
- File categories mapping extensions to target directories with automatic file routing.

#### BitTorrent
- BitTorrent backend integration powered by `librqbit` for magnet links and `.torrent` files.
- Interactive multi-file torrent selection.
- Configurable seeding ratio targets (default 1.0, or 0 to disable).
- DHT, peer exchange (PEX), and UPnP port mapping.
- Torrent state persistence surviving daemon restarts.

#### Media & Streams
- HLS backend for `.m3u8` playlists with stream quality probe, format selection, and AES-128 decryption.
- DASH backend for `.mpd` manifests with multi-track audio and video extraction.
- Automatic audio and video track muxing via `ffmpeg`.
- yt-dlp backend with probe discovery, format selection, child-process progress tracking, and rate limiting.

#### Site Grabber & Plugins
- Site grabber crawler with depth limits, `robots.txt` compliance, duplicate elimination, and link extraction from `<a>`, `<img>`, `<source>`, `<video>`, `<audio>`, and `<link>` HTML tags via `scraper`.
- Out-of-process resolver plugin architecture (`odm-plugin`) with JSON stdin/stdout protocol, 30-second timeout isolation, and process crash containment.
- Plugin manifest parser (`plugin.toml`) supporting wildcard globbing and regex URL patterns.

#### Clients & User Interfaces
- Desktop GUI (`odm-gui`) built with `egui`/`eframe` featuring dark/light themes, segment visualizer, download table, add download dialog with live URL probe, category/queue sidebar, system tray icon, completion notifications, and URL clipboard monitoring.
- Command-line client (`odm`) with human-readable and `--json` output modes for downloads, queues, grabber, and plugin management.
- Headless background daemon (`odm-daemon`) with `axum` HTTP and WebSocket API under `/api/v1`.
- Embedded responsive single-page Web UI (`webui/`) served directly by the daemon at `http://127.0.0.1:6820/` with live WebSocket progress updates and job controls.
- Browser extension for Chromium and Firefox (Manifest V3) with download interception, configurable filters, context menu integration ("Download with odm", "Download all links"), and video media detection (.m3u8, .mpd) with badge count.

#### Packaging & Release CI/CD
- Cross-platform extension packaging script (`scripts/package-extension.mjs`) generating Chrome and Firefox ZIP bundles with manifest transformation and unit test suite.
- GitHub Actions release CI/CD workflow (`.github/workflows/release.yml`) triggered on `v*` tags, building release binaries for Windows (x86_64), Linux (x86_64), macOS (Intel & Apple Silicon), generating `SHA256SUMS.txt`, and publishing release assets.
- Complete documentation overhaul covering installation, usage, architecture, external dependencies, and OS unsigned binary notices for Windows SmartScreen and macOS Gatekeeper.

### Security
- Cryptographic bearer token authentication on all daemon REST endpoints and WebSocket streams.
- Host header validation to prevent DNS rebinding attacks.
- Strict CORS rules scoped to the daemon origin and browser extension.
- Clear security disclosures and UI warnings for un-sandboxed plugin execution.
