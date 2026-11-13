# Milestone 10: Packaging, Documentation, and Release Implementation Plan

> **Specification Reference:** `docs/superpowers/specs/2026-11-11-odm-design.md` (Sections 1, 2, 6, 7, 9, 10, 11, 12).  
> **Milestone Success Criterion:** v0.1.0 binaries published (verified by automated cross-platform release workflow definition, extension packaging script with unit test, comprehensive README and CHANGELOG documentation, full CI pass `cargo test` / `cargo clippy` / `cargo fmt` / `cargo deny`, and manual release checklist `docs/manual-checklists/m10-release.md`).

---

## Architecture and Key Design Decisions

1. **Extension Packaging Automation (`scripts/package-extension.mjs`)**:
   - The `extension/` directory contains shared assets alongside browser-specific manifests (`manifest.chrome.json` and `manifest.firefox.json`).
   - For distribution, users and release archives require standalone, ready-to-load ZIP bundles where each target manifest is renamed to `manifest.json`.
   - A pure Node.js packaging script (`scripts/package-extension.mjs`) builds:
     - `dist/odm-extension-chrome-v0.1.0.zip`: containing `manifest.chrome.json` renamed to `manifest.json`, plus `background.js`, `content.js`, `filter.js`, `popup.html`, `popup.js`, `popup.css`, `options.html`, `options.js`, `options.css`, and `icons/*.png`. Excludes `test/` and `manifest.firefox.json`.
     - `dist/odm-extension-firefox-v0.1.0.zip`: containing `manifest.firefox.json` renamed to `manifest.json`, plus the same shared files. Excludes `test/` and `manifest.chrome.json`.
   - Uses standard Node.js APIs without external npm dependencies.
   - Tested automatically via `node --test scripts/test/package-extension.test.mjs`.

2. **Cross-Platform Release CI/CD Workflow (`.github/workflows/release.yml`)**:
   - Triggered on tag push: `refs/tags/v*` (e.g., `v0.1.0`).
   - Permissions: `contents: write`.
   - Multi-platform matrix build covering all three target operating systems:
     - `windows-latest` -> target `x86_64-pc-windows-msvc` -> packages `odm-v0.1.0-windows-x86_64.zip` containing `odm.exe`, `odm-daemon.exe`, `odm-gui.exe`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`.
     - `ubuntu-latest` -> target `x86_64-unknown-linux-gnu` -> packages `odm-v0.1.0-linux-x86_64.tar.gz` containing `odm`, `odm-daemon`, `odm-gui`, `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`.
     - `macos-13` (Intel) -> target `x86_64-apple-darwin` -> packages `odm-v0.1.0-macos-x86_64.tar.gz`.
     - `macos-latest` (Apple Silicon) -> target `aarch64-apple-darwin` -> packages `odm-v0.1.0-macos-aarch64.tar.gz`.
   - Builds optimized `--release --locked` binaries.
   - Extension packaging job packages the Chrome and Firefox `.zip` archives.
   - Checksum step: generates `SHA256SUMS.txt` for all release archives.
   - Release creation: uses `softprops/action-gh-release@v2` to publish the release with release notes extracted from `CHANGELOG.md` and all binary and extension archives attached.

3. **Release Documentation Overhaul (`README.md`)**:
   - Updates project status from "early development" to official v0.1.0 release.
   - Complete feature walkthrough: segmented downloads, torrents (`librqbit`), HLS/DASH/yt-dlp, plugins, site grabber, Web UI, browser extension, CLI, egui desktop GUI.
   - Installation guides for Windows, Linux, and macOS.
   - **OS Unsigned Binary Notices (Spec Section 9 requirement):**
     - Explicit guidance explaining why Windows SmartScreen flags the executable ("Windows protected your PC") and how to proceed (More info -> Run anyway).
     - Explicit guidance explaining macOS Gatekeeper checks ("cannot be opened because Apple cannot check it for malicious software") and how to permit execution (Control-click Open, or System Settings > Privacy & Security).
     - Linux execution permission guidance (`chmod +x`).
   - Browser extension installation guide (loading unpacked extension in Chrome and Firefox).
   - Embedded Web UI guide (`http://127.0.0.1:6820/`).
   - External dependencies guide (`ffmpeg` and `yt-dlp`).
   - CLI commands reference and examples.
   - Security model and plugin execution disclaimer.

4. **Changelog Finalization (`CHANGELOG.md`)**:
   - Promotes `[Unreleased]` to `## [0.1.0] - 2026-11-13`.
   - Groups all features delivered across M0 through M9 cleanly into standard Keep a Changelog categories (Added, Security).
   - Prepares an empty `[Unreleased]` section for post-v0.1 development.

5. **Milestone 10 Manual Checklist (`docs/manual-checklists/m10-release.md`)**:
   - Formal gate checklist for verifying release artifacts, archive file structures, SHA256 integrity, documentation accuracy, OS warning instructions, CI pass status, and the repository tag release procedure.

---

## Dependencies Plan

No new crate or npm dependencies are introduced in Milestone 10.
All packaging, CI, and documentation tasks use existing workspace crates and standard Node/system utilities.
Verified against `deny.toml` (`cargo deny check`).

---

## Tasks

| Task | Status | Commit | Description |
|---|---|---|---|
| Task 1 | [x] Completed | `dd095cd` | `build(extension): add cross-platform extension packaging script and automated test` |
| Task 2 | [x] Completed | `754caaa` | `ci: add github actions release workflow for cross-platform binaries and extensions` |
| Task 3 | [x] Completed | `82d83e5` | `docs: update readme with v0.1.0 installation, usage guides, and os warning notices` |
| Task 4 | [x] Completed | `8d84bf0` | `docs: cut v0.1.0 release in changelog` |
| Task 5 | [x] Completed | Pending | `docs: add manual release checklist for m10` |

---

### Task 1: Extension Packaging Script & Automation (`scripts/package-extension.mjs`) [Completed]

- **Commit Message:** `build(extension): add cross-platform extension packaging script and automated test`
- **Goal:** Create a standalone, cross-platform packaging script in `scripts/package-extension.mjs` that packages `extension/` into `dist/odm-extension-chrome-v0.1.0.zip` and `dist/odm-extension-firefox-v0.1.0.zip` with manifest substitution and test exclusion. Add automated test `scripts/test/package-extension.test.mjs`.

#### Steps:
1. **Write the test first (`scripts/test/package-extension.test.mjs`):**
   ```javascript
   import { test } from 'node:test';
   import assert from 'node:assert/strict';
   import fs from 'node:fs';
   import path from 'node:path';
   import { execSync } from 'node:child_process';

   test('package-extension creates valid Chrome and Firefox zip bundles', (t) => {
     const outDir = path.resolve('target/test-extension-dist');
     if (fs.existsSync(outDir)) {
       fs.rmSync(outDir, { recursive: true, force: true });
     }

     execSync(`node scripts/package-extension.mjs --out-dir "${outDir}"`, {
       stdio: 'pipe',
       encoding: 'utf8',
     });

     const chromeZip = path.join(outDir, 'odm-extension-chrome-v0.1.0.zip');
     const firefoxZip = path.join(outDir, 'odm-extension-firefox-v0.1.0.zip');

     assert.ok(fs.existsSync(chromeZip), 'Chrome extension zip must exist');
     assert.ok(fs.existsSync(firefoxZip), 'Firefox extension zip must exist');

     assert.ok(fs.statSync(chromeZip).size > 1000, 'Chrome extension zip must not be empty');
     assert.ok(fs.statSync(firefoxZip).size > 1000, 'Firefox extension zip must not be empty');

     // Verify staging directories content
     const chromeStage = path.join(outDir, 'chrome');
     const firefoxStage = path.join(outDir, 'firefox');

     assert.ok(fs.existsSync(path.join(chromeStage, 'manifest.json')), 'Chrome stage must have manifest.json');
     assert.ok(fs.existsSync(path.join(firefoxStage, 'manifest.json')), 'Firefox stage must have manifest.json');

     const chromeManifest = JSON.parse(fs.readFileSync(path.join(chromeStage, 'manifest.json'), 'utf8'));
     assert.equal(chromeManifest.name, 'Open Download Manager');
     assert.ok(chromeManifest.background.service_worker, 'Chrome manifest must have service_worker');

     const firefoxManifest = JSON.parse(fs.readFileSync(path.join(firefoxStage, 'manifest.json'), 'utf8'));
     assert.equal(firefoxManifest.name, 'Open Download Manager');
     assert.ok(firefoxManifest.background.scripts, 'Firefox manifest must have background.scripts');

     // Ensure test files are not packaged
     assert.ok(!fs.existsSync(path.join(chromeStage, 'test')), 'Chrome stage must not contain test folder');
     assert.ok(!fs.existsSync(path.join(firefoxStage, 'test')), 'Firefox stage must not contain test folder');
   });
   ```

2. **Run test to verify failure:**
   ```powershell
   node --test scripts/test/package-extension.test.mjs
   ```
   *Expected:* Failure (script does not exist yet).

3. **Implement `scripts/package-extension.mjs`:**
   - Creates output directory (`dist` by default, or `--out-dir`).
   - Copies extension source files into `chrome` and `firefox` staging directories.
   - Renames `manifest.chrome.json` -> `chrome/manifest.json`.
   - Renames `manifest.firefox.json` -> `firefox/manifest.json`.
   - Uses Node built-in ZIP compression or cross-platform archiving (`tar -a -cf` / PowerShell `Compress-Archive` / zip utility) to generate `.zip` archives.
   - Cleans up temporary staging files or retains them in target directory.

4. **Run test and verify pass:**
   ```powershell
   node --test scripts/test/package-extension.test.mjs
   ```
   *Expected:* `tests 1, pass 1, fail 0`.

5. **Verify codebase gates:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

### Task 2: GitHub Actions Release Workflow (`.github/workflows/release.yml`) [Completed]

- **Commit Message:** `ci: add github actions release workflow for cross-platform binaries and extensions`
- **Goal:** Create `.github/workflows/release.yml` triggered on git tags matching `v*` to automatically build optimized release binaries for Windows, Linux, and macOS, package extension zips, generate `SHA256SUMS.txt`, and publish a GitHub Release.

#### Steps:
1. **Create `.github/workflows/release.yml`:**
   - `on: push: tags: ['v*']`
   - Permissions: `contents: write`
   - Job 1: `build-binaries`
     - Matrix strategy:
       - `target: x86_64-pc-windows-msvc`, `os: windows-latest`, `archive_ext: zip`
       - `target: x86_64-unknown-linux-gnu`, `os: ubuntu-latest`, `archive_ext: tar.gz`
       - `target: x86_64-apple-darwin`, `os: macos-13`, `archive_ext: tar.gz`
       - `target: aarch64-apple-darwin`, `os: macos-latest`, `archive_ext: tar.gz`
     - Steps:
       - Checkout code (`actions/checkout@v7`).
       - Install pinned toolchain (`rustup toolchain install`).
       - Build release binaries: `cargo build --workspace --release --locked --target ${{ matrix.target }}`.
       - Assemble archive: bundles `odm`, `odm-daemon`, `odm-gui` (with `.exe` extension on Windows) plus `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`.
       - Archive naming: `odm-${{ github.ref_name }}-${{ matrix.target }}.${{ matrix.archive_ext }}`.
       - Upload artifact (`actions/upload-artifact@v4`).
   - Job 2: `build-extensions`
     - Runs on `ubuntu-latest`.
     - Executes `node scripts/package-extension.mjs --out-dir dist`.
     - Uploads `odm-extension-chrome-*.zip` and `odm-extension-firefox-*.zip`.
   - Job 3: `publish-release`
     - Needs: `[build-binaries, build-extensions]`.
     - Runs on `ubuntu-latest`.
     - Downloads all artifacts into a staging directory.
     - Computes SHA256 checksums: `sha256sum * > SHA256SUMS.txt`.
     - Uses `softprops/action-gh-release@v2` with `files: [...]` to publish all binary archives, extension zips, and `SHA256SUMS.txt`.
2. **Verify syntax:**
   - Parse YAML with a node script or powershell to ensure valid YAML structure without syntax errors.
3. **Verify codebase gates:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

### Task 3: Documentation Updates for v0.1.0 Release (`README.md`) [Completed]

- **Commit Message:** `docs: update readme with v0.1.0 installation, usage guides, and os warning notices`
- **Goal:** Update `README.md` to reflect v0.1.0 release status, provide end-to-end installation and usage instructions across GUI, CLI, Web UI, and browser extension, document external tools (`ffmpeg`, `yt-dlp`), and clearly document the OS unsigned binary warnings (Spec Section 9).

#### Steps:
1. **Update `README.md` sections:**
   - **Header & Badges:** Update to v0.1.0 release status.
   - **Feature Overview:** Segmented HTTP/HTTPS transfer, BitTorrent, HLS/DASH/yt-dlp video, plugins, site grabber, Web UI, Chrome & Firefox extension, egui desktop GUI.
   - **Installation:**
     - Download prebuilt archives from GitHub Releases.
     - Windows: Extract zip, run `odm-gui.exe` (or `odm.exe` / `odm-daemon.exe`).
     - Linux: Extract tar.gz, `chmod +x odm*`, launch desktop GUI or daemon.
     - macOS: Extract tar.gz, run binaries.
   - **OS Unsigned Binary Notices (Spec Section 9 requirement):**
     - Windows SmartScreen: Explain that v0.1.0 binaries are open-source and unsigned; when Windows Defender SmartScreen displays "Windows protected your PC", click **More info** -> **Run anyway**.
     - macOS Gatekeeper: Explain that v0.1.0 binaries are not notarized with an Apple Developer certificate; if macOS reports "cannot be opened because Apple cannot check it for malicious software", right-click (Control-click) the application, select **Open**, and confirm in the dialog (or allow under **System Settings > Privacy & Security**).
   - **Browser Extension Setup:**
     - Chrome: Extract `odm-extension-chrome-v0.1.0.zip`, go to `chrome://extensions/`, enable Developer Mode, click "Load unpacked", select folder. Configure token and daemon port (6820) in Options.
     - Firefox: Open `about:debugging#/runtime/this-firefox`, click "Load Temporary Add-on", select `manifest.json` from extracted `odm-extension-firefox-v0.1.0.zip`.
   - **Web UI:**
     - Access `http://127.0.0.1:6820/` in any desktop or mobile browser. Enter token from `<config_dir>/odm/token`.
   - **CLI Reference:**
     - Common commands: `odm add <url>`, `odm list`, `odm info <id>`, `odm pause|resume|cancel <id>`, `odm grab <url>`, `odm plugin list`, `odm queue`, `odm daemon status`.
   - **External Dependencies:**
     - `ffmpeg`: Required for DASH stream muxing and optional for HLS remuxing.
     - `yt-dlp`: Supported video sites download helper (downloadable via GUI or configurable in settings).
   - **Security Notice:**
     - Explains token authentication, loopback host validation, CORS policies, and plugin execution permissions.
2. **Review formatting:**
   - Verify all links and markdown formatting.
3. **Verify codebase gates:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

### Task 4: Changelog Finalization for v0.1.0 (`CHANGELOG.md`) [Completed]

- **Commit Message:** `docs: cut v0.1.0 release in changelog`
- **Goal:** Promote `[Unreleased]` in `CHANGELOG.md` to `## [0.1.0] - 2026-11-13`, detailing all delivered capabilities from M0 through M9, and add an empty `## [Unreleased]` section at the top.

#### Steps:
1. **Update `CHANGELOG.md`:**
   - Add empty `## [Unreleased]` section with subsections.
   - Add `## [0.1.0] - 2026-11-13` containing all features added in M0-M9 organized by area:
     - Core Engine & Transfers
     - Torrents
     - Media & Streams
     - Plugins & Site Grabber
     - Clients (GUI, CLI, Web UI)
     - Browser Extension
     - Security & Networking
     - Infrastructure & CI
2. **Verify codebase gates:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

### Task 5: Milestone 10 Manual Verification Checklist (`docs/manual-checklists/m10-release.md`) [Completed]

- **Commit Message:** `docs: add manual release checklist for m10`
- **Goal:** Create `docs/manual-checklists/m10-release.md` providing a structured verification checklist for the v0.1.0 release gate.

#### Steps:
1. **Create `docs/manual-checklists/m10-release.md`:**
   - Section 1: Release Build Verification (`cargo build --workspace --release --locked`).
   - Section 2: Extension Packaging Verification (`node scripts/package-extension.mjs`).
   - Section 3: Documentation & OS Warning Checks (`README.md` and `CHANGELOG.md`).
   - Section 4: CI & Static Analysis Quality Gate (`fmt`, `clippy`, `test`, `deny`, `node --test`).
   - Section 5: Release Tag and Publishing Procedure (for repository owner).
2. **Verify codebase gates:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

## What Is Left After Milestone 10

Once Milestone 10 is completed, the codebase is at version **v0.1.0** and all design requirements from `docs/superpowers/specs/2026-11-11-odm-design.md` are satisfied.

The remaining actions fall into three categories:

### 1. Maintainer Release Actions (Manual Operator Steps)
Per Rule 12 of `AGENTS.md` ("*Never push, create a repository, create a tag, publish a crate or change GitHub settings*"), coding agents do not execute remote git push or tag actions. The human repository owner performs:
- **Pushing commits to remote:** `git push origin main` (pushing the complete milestone history).
- **Creating and pushing the release tag:** `git tag -a v0.1.0 -m "Release v0.1.0" && git push origin v0.1.0`.
- **Verifying GitHub Actions Release:** Monitoring the `release.yml` workflow run on GitHub to confirm binary builds, extension zips, and checksums are published to GitHub Releases.

### 2. Distribution & Registry Publishing (Post-Release Distribution)
- **Browser Web Stores:**
  - Submit `odm-extension-chrome-v0.1.0.zip` to the **Chrome Web Store** developer dashboard for store listing.
  - Submit `odm-extension-firefox-v0.1.0.zip` to **addons.mozilla.org (AMO)** for signing and listing.
- **System Package Managers:**
  - Windows: Submit an official `winget` manifest or Scoop manifest for `odm`.
  - macOS / Linux: Create a Homebrew formula/cask (`brew install odm`), Arch Linux AUR package (`odm-bin`), or Debian `.deb`.
- **Crates.io:** Optionally publish crates (`odm-core`, `odm-api`, `odm-cli`, etc.) if public crate distribution is desired.

### 3. Future Roadmap (Post-v0.1 Non-Goals from Spec Section 1)
Future milestones beyond v0.1.0 can address the explicitly deferred non-goals:
- **Code Signing Certificates:** Acquiring a Windows Authenticode code signing certificate (to eliminate SmartScreen warnings) and Apple Developer ID signing/notarization (to eliminate Gatekeeper warnings).
- **FTP & SFTP Support:** Classic FTP/FTPS and SSH SFTP protocols as additional `Backend` implementations.
- **Plugin Sandbox & Marketplace:** Sandboxed plugin execution (WASM or restricted subprocesses) and in-app plugin installation.
- **Automatic In-App Updates:** Background update checker and updater for daemon, GUI, and CLI.
- **Daemon Native TLS:** Built-in TLS encryption for the daemon's remote HTTP/WebSocket server.
- **Live Streams:** Capturing live streaming HLS/DASH broadcasts.
- **Internationalization (i18n):** Translating the GUI, Web UI, and extension into multiple languages.
- **Mobile Apps & Cloud Sync:** Native iOS and Android companion apps and encrypted peer-to-peer job synchronization.
