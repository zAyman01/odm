# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Workspace skeleton, CI and repository files.
- Core job model, state machine, SQLite persistence and single-stream HTTP downloads.
- Segmented downloads with dynamic splitting, resume, retry, speed limits and queues.
- Daemon REST API and WebSocket event streaming under `/api/v1` with bearer authentication, host header rebinding protection and CORS.
- Command-line client (`odm`) with human-readable and `--json` output modes for download management and daemon control.
- Desktop graphical user interface (`odm-gui`) built with `eframe`/`egui` featuring download list, toolbar controls, category/queue sidebar, segment visualizer detail panel, add download dialog with live URL probe, settings dialog, dark/light theme, clipboard URL monitoring, and completion notifications.
- Cryptographic checksum verification (SHA-256, SHA-1, MD5) with streaming integrity checks in `Verifying` state.
- Multi-mirror downloads with dynamic segment distribution and automatic mirror failover.
- Metalink 3 and Meta4 XML file parser.
- Batch URL pattern expansion supporting numeric (`[001-100]`) and alphabetic ranges with Cartesian products.
- File categories mapping extensions to target directories with automatic file routing.
- Queue scheduling with configurable start/stop times and active weekdays.
- Full CLI and GUI controls for checksums, mirrors, categories, batch URLs, and schedules.
