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
