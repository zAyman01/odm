# Milestone 10: Packaging, Documentation, and Release Manual Checklist

Verification checklist for odm Milestone 10 covering release builds, packaging automation, documentation, security checks, and repository release procedures for **v0.1.0**.

---

## 1. Release Binary Builds

- [ ] Execute clean release build:
  ```powershell
  cargo build --workspace --release --locked
  ```
- [ ] Verify binary output existence and permissions:
  - `target/release/odm` (or `odm.exe` on Windows)
  - `target/release/odm-daemon` (or `odm-daemon.exe` on Windows)
  - `target/release/odm-gui` (or `odm-gui.exe` on Windows)
- [ ] Test CLI basic command execution:
  ```powershell
  ./target/release/odm --help
  ./target/release/odm daemon status
  ```
- [ ] Test GUI executable launch and verify main window renders with dark/light themes.
- [ ] Test Daemon executable launch and verify `GET http://127.0.0.1:6820/api/v1/health` returns `200 OK`.

---

## 2. Browser Extension Packaging

- [ ] Execute extension packaging script:
  ```powershell
  node scripts/package-extension.mjs --out-dir dist
  ```
- [ ] Verify generated ZIP archives in `dist/`:
  - `dist/odm-extension-chrome-v0.1.0.zip`
  - `dist/odm-extension-firefox-v0.1.0.zip`
- [ ] Verify Chrome extension ZIP bundle:
  - Contains `manifest.json` with `"service_worker": "background.js"`
  - Contains `background.js`, `content.js`, `filter.js`
  - Contains `popup.html`, `popup.js`, `popup.css`
  - Contains `options.html`, `options.js`, `options.css`
  - Contains `icons/icon16.png`, `icons/icon48.png`, `icons/icon128.png`
  - Does NOT contain `test/` or `manifest.firefox.json`
- [ ] Verify Firefox extension ZIP bundle:
  - Contains `manifest.json` with `"scripts": ["background.js"]`
  - Contains all shared scripts, styles, options, and icons
  - Does NOT contain `test/` or `manifest.chrome.json`
- [ ] Test unpacking and loading extension into Chrome (`chrome://extensions/`) and Firefox (`about:debugging`).

---

## 3. Documentation & OS Unsigned Binary Notices

- [ ] Inspect `README.md`:
  - Status is updated to `v0.1.0 Released`.
  - Feature list matches all implemented capabilities (segmented HTTP, torrents, HLS/DASH/yt-dlp, plugins, grabber, GUI, CLI, Web UI, extension).
  - Clear installation instructions for Windows, Linux, and macOS.
  - **Windows SmartScreen Notice:** Explains why Windows Defender displays "Windows protected your PC" for unsigned v0.1.0 binaries and provides clear instructions: click **More info** -> **Run anyway**.
  - **macOS Gatekeeper Notice:** Explains why Gatekeeper reports "cannot be opened because Apple cannot check it for malicious software" and provides clear instructions: Control-click **Open** or allow in **System Settings > Privacy & Security**.
  - Browser extension setup guide for Chrome and Firefox.
  - External tool requirements documented (`ffmpeg` and `yt-dlp`).
  - Security model notice (token auth, loopback host validation, plugin security disclaimer).
- [ ] Inspect `CHANGELOG.md`:
  - `[0.1.0] - 2026-11-13` release section is present with comprehensive categorized notes.
  - Fresh empty `[Unreleased]` section is ready at top for future work.

---

## 4. Quality & Security Gates

Execute all project test and static analysis gates:

- [ ] Formatting:
  ```powershell
  cargo fmt --all --check
  ```
- [ ] Clippy (zero warnings allowed):
  ```powershell
  cargo clippy --workspace --all-targets -- -D warnings
  ```
- [ ] Full workspace test suite:
  ```powershell
  cargo test --workspace
  ```
- [ ] Dependency & license audit:
  ```powershell
  cargo deny check
  ```
- [ ] Extension unit tests:
  ```powershell
  node --test extension/test/filter.test.js
  node --test scripts/test/package-extension.test.mjs
  ```

---

## 5. Repository Maintainer Release Procedure

Per Rule 12 of `AGENTS.md`, the coding agent never pushes to remote, creates tags, or publishes crates. The human repository owner completes the release with:

1. **Review local git history:**
   ```powershell
   git status
   git log --oneline -n 15
   ```
2. **Push main branch to GitHub:**
   ```powershell
   git push origin main
   ```
3. **Tag the v0.1.0 release:**
   ```powershell
   git tag -a v0.1.0 -m "Release v0.1.0"
   git push origin v0.1.0
   ```
4. **Monitor Release Workflow:**
   - Open GitHub repository Actions tab: `.github/workflows/release.yml`.
   - Verify `build-binaries` matrix passes for Windows (`x86_64-pc-windows-msvc`), Linux (`x86_64-unknown-linux-gnu`), macOS Intel (`x86_64-apple-darwin`), and macOS Apple Silicon (`aarch64-apple-darwin`).
   - Verify `build-extensions` packages Chrome and Firefox zips.
   - Verify `publish-release` computes `SHA256SUMS.txt` and publishes the GitHub Release with attached assets.
