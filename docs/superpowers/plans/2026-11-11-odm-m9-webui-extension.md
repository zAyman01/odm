# Milestone 9: Web UI and Browser Extension Implementation Plan

> **Specification Reference:** `docs/superpowers/specs/2026-11-11-odm-design.md` (Sections 2, 5, 6, 8, 12).  
> **Milestone Success Criterion:** Browser download is captured; phone controls a job (verified by automated tests across `odm-daemon`, `odm-api`, and `extension/`, plus manual checklist `docs/manual-checklists/m9-webui-extension.md`).

---

## Architecture and Key Design Decisions

1. **Web UI (`webui/`)**:
   - One static page: `webui/index.html` containing all HTML, CSS and vanilla JavaScript (no build step, no framework).
   - Embedded into the `odm-daemon` binary using `rust-embed` so the daemon serves it directly.
   - Features:
     - **Token entry.** On first load, prompts for the bearer token. Stores it in `localStorage`. A "Disconnect" button clears it.
     - **Job list.** Table of downloads with columns: filename, size, progress bar, speed, ETA, state. Refreshes via WebSocket events, with initial load from `GET /api/v1/downloads`.
     - **Add URL.** An "Add" button opens a form for URL input, submits `POST /api/v1/downloads`.
     - **Job controls.** Per-job buttons: pause, resume, cancel, remove. No settings, no grabber (spec: "No settings, no grabber").
     - **Live progress.** WebSocket connection to `/api/v1/events` using the stored token in the first message. Receives progress, state change, added, and removed events.
     - **Responsive.** Usable on a phone browser (the "phone controls a job" criterion).
   - No `<script>` external sources. No CDN. Everything self-contained.

2. **Daemon Web UI Serving (`odm-daemon`)**:
   - `rust-embed` (version `8.13.0`) with `#[folder = "../../webui/"]` embeds the `webui/` directory at compile time.
   - A fallback route serves `index.html` for `/` and any non-API path. Static assets served with correct MIME types.
   - The Web UI route does **not** require bearer auth (it's a static page; auth is done by the JS calling the API with the token). However, all API calls from the Web UI still require the token.
   - CORS update: add `null` origin (for `file://` access during development) — no, the Web UI is served from the daemon's own origin, so same-origin requests work without CORS. No CORS change needed.
   - Auth bypass: the Web UI HTML/JS/CSS is served without auth, but API routes remain protected. The existing `/health` bypass pattern is extended to cover static asset paths.

3. **Browser Extension (`extension/`)**:
   - Manifest V3, plain JavaScript, no build step.
   - **One codebase, two manifests:**
     - `extension/manifest.chrome.json` — Chrome Manifest V3 with `service_worker` background.
     - `extension/manifest.firefox.json` — Firefox Manifest V3 with `background.scripts`.
   - Shared source files:
     - `extension/background.js` — Service worker / background script: download interception, context menu, media detection, communication with daemon.
     - `extension/popup.html` + `extension/popup.js` + `extension/popup.css` — Toolbar popup: media list, connection status.
     - `extension/options.html` + `extension/options.js` + `extension/options.css` — Options page: token, port, filters.
     - `extension/content.js` — Content script for "Download all links" context menu (collects links from the page).
     - `extension/filter.js` — Pure-function filter logic: minimum file size, extension allow/block list, per-site disable. Exported for unit testing.
     - `extension/icons/` — Extension icons (16, 48, 128 px).
   - **Download Capture** (`chrome.downloads.onDeterminingFilename` or `browser.downloads.onCreated`):
     - Intercepts browser-initiated downloads.
     - Checks filters (min size, extension list, per-site disable).
     - If accepted: cancels the browser download, sends `POST /api/v1/downloads` to daemon with URL, cookies (from `chrome.cookies.getAll`), referer, and user-agent.
     - If daemon is unreachable: lets the browser download proceed (fallback).
   - **Context Menu:**
     - "Download with odm" on links (`contexts: ["link"]`).
     - "Download all links" on pages (`contexts: ["page"]`) — invokes content script to collect `<a>` href values, sends them in batch.
   - **Media Detection:**
     - Uses `chrome.webRequest.onBeforeRequest` to observe `.m3u8`, `.mpd`, and video content-type requests.
     - Badge count on toolbar icon shows detected media count per tab.
     - Popup lists detected media URLs with "Send to odm" buttons.
   - **Options:**
     - Stored in `chrome.storage.sync` / `browser.storage.sync`.
     - Fields: `token`, `port` (default 6820), `minSize` (KB), `extensionList` (comma-separated), `disabledSites` (newline-separated hostnames).
   - **Privacy:** Cookies and URLs are sent only to the local daemon (`http://127.0.0.1:<port>`).
   - **Fallback:** If `POST` to daemon fails (network error), the browser download is not cancelled.

4. **Testing Strategy:**
   - **Web UI:** Tested as embedded static files via the daemon's test router. An integration test fetches `/` and verifies it returns HTML with expected markers, and verifies API interactions work from the same origin.
   - **Extension filter logic:** Pure JavaScript functions in `extension/filter.js`, unit-tested with `node --test` in `extension/test/filter.test.js`.
   - **Extension capture logic:** Tested via manual checklist (browser APIs are not available in Node).

---

## Dependencies Plan

| Crate | Dependency | Version | Purpose |
|---|---|---|---|
| `odm-daemon` | `rust-embed` | `8.13.0` | Embedding `webui/` static files into the daemon binary |
| `odm-daemon` | `mime_guess` | `2.0.5` | Detecting MIME types for embedded static files |

No new npm/JS dependencies for the extension or web UI (plain vanilla JS, `node --test` is built in).

---

## Tasks

| Task | Status | Commit | Description |
|---|---|---|---|
| Task 1 | [x] Completed | `780f453` | `feat(webui): create static web ui page with job list, add url, and live progress` |
| Task 2 | [x] Completed | `6bb7dc9` | `feat(daemon): embed web ui with rust-embed and serve static files` |
| Task 3 | [x] Completed | `0cc98e1` | `feat(extension): implement filter logic with node test suite` |
| Task 4 | [x] Completed | `3e00ad1` | `feat(extension): create chrome and firefox manifests with shared background and popup` |
| Task 5 | [x] Completed | `534a59a` | `feat(extension): implement download capture, context menu, and media detection` |
| Task 6 | [x] Completed | `69a0ca6` | `feat(extension): implement options page for token, port, and filter configuration` |
| Task 7 | [x] Completed | Pending | `docs: record M9 web ui and browser extension` |

### Task 1: Static Web UI Page (`webui/`) [Pending]

- **Commit Message:** `feat(webui): create static web ui page with job list, add url, and live progress`
- **Goal:** Create the `webui/index.html` file — a self-contained single page with HTML, CSS, and vanilla JavaScript. Provides token entry, job list with live progress, add URL, and pause/resume/cancel/remove controls. Responsive for phone browsers.

#### Steps:
1. **Create `webui/index.html`:**
   One file containing all HTML, `<style>`, and `<script>` sections:
   - **Token Entry Screen:**
     - On load, check `localStorage.getItem('odm_token')`.
     - If missing: show a centered card with a password input and "Connect" button.
     - On submit: call `GET /api/v1/health` to verify connectivity, then attempt `GET /api/v1/downloads` with `Authorization: Bearer <token>`.
     - On success: store token, show main screen.
     - On failure: show error message inline.
   - **Main Screen (after auth):**
     - **Header:** "odm" branding, version (from `/api/v1/health`), "Add" button, "Disconnect" button.
     - **Add form:** Slide-down or modal. URL text input. "Add" button calls `POST /api/v1/downloads` with `{ "url": "<value>" }`. Shows success/error feedback.
     - **Job table:**
       - Columns: Filename, Size, Progress (bar + percentage), Speed, State.
       - ETA computed client-side: `(total - downloaded) / speed`.
       - Rows are clickable (no detail panel, just action buttons).
     - **Per-job actions:** Pause, Resume, Cancel, Remove buttons per row, calling `POST /api/v1/downloads/{id}/pause|resume|cancel` and `DELETE /api/v1/downloads/{id}`.
     - **Status bar:** Connection indicator (green dot when WebSocket is open).
   - **WebSocket:**
     - Connect to `ws://<host>/api/v1/events`.
     - First message: `{"token":"<stored_token>"}`.
     - On `{"authenticated":true}`: listen for events.
     - Event types handled:
       - `job_added` → add row to table.
       - `job_state_changed` → update state cell.
       - `job_progress` → update progress bar, speed, ETA.
       - `job_removed` → remove row from table.
     - On close: show disconnected indicator, attempt reconnect with exponential backoff (1s, 2s, 4s, up to 30s).
   - **CSS:**
     - Dark theme by default with `prefers-color-scheme` media query for light.
     - Mobile-first responsive: table stacks to cards below 600px width.
     - Progress bar: `<div>` with percentage width, colored by state (blue=downloading, green=completed, yellow=paused, red=failed).
   - **Utility functions:**
     - `formatBytes(bytes)` → "1.2 MB", "843 KB", etc.
     - `formatSpeed(bps)` → "8.3 MB/s".
     - `formatEta(seconds)` → "2m 15s", "1h 3m".

2. **Verify Manually:**
   Open `webui/index.html` directly in a browser to check layout renders (API calls will fail, but layout is visible).

3. **Commit:**
   ```powershell
   git add webui/index.html
   git commit -m "feat(webui): create static web ui page with job list, add url, and live progress"
   ```

---

### Task 2: Embed Web UI with `rust-embed` and Serve Static Files (`odm-daemon`) [Pending]

- **Commit Message:** `feat(daemon): embed web ui with rust-embed and serve static files`
- **Goal:** Add `rust-embed = "8.13.0"` and `mime_guess = "2.0.7"` to `odm-daemon`, embed `webui/` assets, and serve them as a fallback route. Write integration tests verifying the web UI is accessible and API works from same origin.

#### Steps:
1. **Write Test First:**
   Create `crates/odm-daemon/tests/webui.rs` starting with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
   - Test 1: `GET /` returns `200 OK` with `Content-Type: text/html` and body containing `<title>odm</title>`.
   - Test 2: `GET /nonexistent-path` returns `200 OK` with `text/html` (SPA fallback to `index.html`).
   - Test 3: `GET /api/v1/health` still returns JSON health response (API routes are not intercepted by web UI).
   - Test 4: `GET /api/v1/downloads` without auth still returns `401 Unauthorized` (web UI serving doesn't bypass API auth).

2. **Run Test to Fail:**
   ```powershell
   cargo test -p odm-daemon --test webui
   ```
   *Expected:* Test fails because `/` returns 404 (no web UI route yet).

3. **Implement Code:**
   - Add dependencies to `crates/odm-daemon/Cargo.toml`:
     ```toml
     rust-embed = "8.13.0"
     mime_guess = "2.0.7"
     ```
   - Run `cargo deny check`.
   - Create `crates/odm-daemon/src/webui.rs`:
     ```rust
     //! Embedded web UI static file serving.
     use axum::http::{StatusCode, header};
     use axum::response::{IntoResponse, Response};
     use rust_embed::Embed;

     #[derive(Embed)]
     #[folder = "../../webui/"]
     struct WebUiAssets;

     pub async fn webui_handler(
         axum::extract::Path(path): axum::extract::Path<String>,
     ) -> impl IntoResponse {
         serve_file(&path)
     }

     pub async fn webui_index_handler() -> impl IntoResponse {
         serve_file("index.html")
     }

     fn serve_file(path: &str) -> Response {
         match WebUiAssets::get(path) {
             Some(content) => {
                 let mime = mime_guess::from_path(path)
                     .first_or_text_plain()
                     .to_string();
                 (
                     StatusCode::OK,
                     [(header::CONTENT_TYPE, mime)],
                     content.data.to_vec(),
                 )
                     .into_response()
             }
             None => {
                 // SPA fallback: serve index.html for unknown paths
                 match WebUiAssets::get("index.html") {
                     Some(content) => (
                         StatusCode::OK,
                         [(header::CONTENT_TYPE, "text/html".to_string())],
                         content.data.to_vec(),
                     )
                         .into_response(),
                     None => StatusCode::NOT_FOUND.into_response(),
                 }
             }
         }
     }
     ```
   - In `crates/odm-daemon/src/lib.rs`: add `pub mod webui;`.
   - In `crates/odm-daemon/src/server.rs`:
     - Import `crate::webui::{webui_handler, webui_index_handler}`.
     - In `build_router_with_state`, after nesting `/api/v1`, add:
       ```rust
       .route("/", get(webui_index_handler))
       .route("/{*path}", get(webui_handler))
       ```
       These go **after** the `/api/v1` nest and `/health` route so API routes take precedence.
   - Update auth bypass in `crates/odm-daemon/src/auth.rs` `require_bearer_token`:
     - Add bypass for paths that don't start with `/api/` (static asset paths). Keep requiring auth for all `/api/v1/*` routes except `/api/v1/health` and `/api/v1/events`.
     ```rust
     // Static web UI paths don't require auth
     if !path.starts_with("/api/") {
         return Ok(next.run(req).await);
     }
     ```

4. **Run Test to Pass:**
   ```powershell
   cargo test -p odm-daemon --test webui
   ```
   *Expected:* All web UI tests pass.

5. **Verify:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   ```

6. **Commit:**
   ```powershell
   git add crates/odm-daemon/Cargo.toml Cargo.lock crates/odm-daemon/src/webui.rs crates/odm-daemon/src/lib.rs crates/odm-daemon/src/server.rs crates/odm-daemon/src/auth.rs crates/odm-daemon/tests/webui.rs
   git commit -m "feat(daemon): embed web ui with rust-embed and serve static files"
   ```

---

### Task 3: Extension Filter Logic with Node Tests (`extension/`) [Pending]

- **Commit Message:** `feat(extension): implement filter logic with node test suite`
- **Goal:** Implement the pure-function filter module `extension/filter.js` and unit test it with `node --test` in `extension/test/filter.test.js`. This module determines whether a download should be intercepted or passed through.

#### Steps:
1. **Write Test First:**
   Create `extension/test/filter.test.js`:
   ```javascript
   import { describe, it } from 'node:test';
   import assert from 'node:assert/strict';
   import { shouldIntercept } from '../filter.js';

   describe('shouldIntercept', () => {
     const defaults = {
       enabled: true,
       minSizeBytes: 1024,
       extensionList: ['zip', 'exe', 'pdf', 'mp4', 'iso'],
       disabledSites: [],
     };

     it('intercepts a matching extension above min size', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/file.zip', 50000, 'example.com', defaults),
         true
       );
     });

     it('ignores a file below minimum size', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/tiny.zip', 500, 'example.com', defaults),
         false
       );
     });

     it('ignores a non-matching extension', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/page.html', 50000, 'example.com', defaults),
         false
       );
     });

     it('ignores when site is disabled', () => {
       const opts = { ...defaults, disabledSites: ['example.com'] };
       assert.strictEqual(
         shouldIntercept('https://example.com/file.zip', 50000, 'example.com', opts),
         false
       );
     });

     it('ignores when interception is globally disabled', () => {
       const opts = { ...defaults, enabled: false };
       assert.strictEqual(
         shouldIntercept('https://example.com/file.zip', 50000, 'example.com', opts),
         false
       );
     });

     it('intercepts when size is unknown (0) and extension matches', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/file.exe', 0, 'example.com', defaults),
         true
       );
     });

     it('extracts extension case-insensitively', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/FILE.ZIP', 50000, 'example.com', defaults),
         true
       );
     });

     it('handles URLs with query strings', () => {
       assert.strictEqual(
         shouldIntercept('https://example.com/file.pdf?token=abc', 50000, 'example.com', defaults),
         true
       );
     });

     it('intercepts with empty extension list (all extensions)', () => {
       const opts = { ...defaults, extensionList: [] };
       assert.strictEqual(
         shouldIntercept('https://example.com/anything.xyz', 50000, 'example.com', opts),
         true
       );
     });

     it('handles subdomain matching for disabled sites', () => {
       const opts = { ...defaults, disabledSites: ['example.com'] };
       assert.strictEqual(
         shouldIntercept('https://sub.example.com/file.zip', 50000, 'sub.example.com', opts),
         true
       );
     });
   });
   ```

2. **Run Test to Fail:**
   ```powershell
   node --test extension/test/filter.test.js
   ```
   *Expected:* Error — `filter.js` not found or `shouldIntercept` not exported.

3. **Implement Code:**
   Create `extension/filter.js`:
   ```javascript
   /**
    * Pure filter logic for the odm browser extension.
    * Determines whether a browser download should be intercepted and sent to odm.
    *
    * @param {string} url - The download URL.
    * @param {number} fileSize - Known file size in bytes (0 if unknown).
    * @param {string} hostname - The hostname of the page that triggered the download.
    * @param {object} opts - Filter options.
    * @param {boolean} opts.enabled - Whether interception is globally enabled.
    * @param {number} opts.minSizeBytes - Minimum file size to intercept (0 = no minimum).
    * @param {string[]} opts.extensionList - File extensions to intercept (empty = all).
    * @param {string[]} opts.disabledSites - Hostnames where interception is disabled.
    * @returns {boolean} Whether the download should be intercepted.
    */
   export function shouldIntercept(url, fileSize, hostname, opts) {
     if (!opts.enabled) return false;

     // Check disabled sites (exact match only)
     if (opts.disabledSites.some(site => hostname === site)) return false;

     // Check minimum size (0 = unknown, always intercept; skip check for unknown)
     if (fileSize > 0 && opts.minSizeBytes > 0 && fileSize < opts.minSizeBytes) return false;

     // Check extension filter (empty list = all extensions)
     if (opts.extensionList.length > 0) {
       const urlPath = url.split('?')[0].split('#')[0];
       const lastSegment = urlPath.split('/').pop() || '';
       const dotIndex = lastSegment.lastIndexOf('.');
       if (dotIndex === -1) return false;
       const ext = lastSegment.substring(dotIndex + 1).toLowerCase();
       if (!opts.extensionList.some(e => e.toLowerCase() === ext)) return false;
     }

     return true;
   }
   ```

4. **Run Test to Pass:**
   ```powershell
   node --test extension/test/filter.test.js
   ```
   *Expected:* All 10 tests pass.

5. **Commit:**
   ```powershell
   git add extension/filter.js extension/test/filter.test.js
   git commit -m "feat(extension): implement filter logic with node test suite"
   ```

---

### Task 4: Chrome and Firefox Manifests with Shared Background and Popup (`extension/`) [Pending]

- **Commit Message:** `feat(extension): create chrome and firefox manifests with shared background and popup`
- **Goal:** Create the two Manifest V3 files, the popup HTML/JS/CSS, and the icon placeholders. The popup shows connection status, detected media count, and links to the options page.

#### Steps:
1. **Create `extension/manifest.chrome.json`:**
   ```json
   {
     "manifest_version": 3,
     "name": "odm — Open Download Manager",
     "version": "0.1.0",
     "description": "Captures browser downloads and sends them to the odm daemon for segmented, resumable downloading.",
     "permissions": [
       "downloads",
       "cookies",
       "contextMenus",
       "webRequest",
       "storage",
       "activeTab",
       "tabs"
     ],
     "host_permissions": [
       "http://127.0.0.1:*/*",
       "http://localhost:*/*",
       "<all_urls>"
     ],
     "background": {
       "service_worker": "background.js",
       "type": "module"
     },
     "action": {
       "default_popup": "popup.html",
       "default_icon": {
         "16": "icons/icon16.png",
         "48": "icons/icon48.png",
         "128": "icons/icon128.png"
       }
     },
     "options_ui": {
       "page": "options.html",
       "open_in_tab": true
     },
     "content_scripts": [
       {
         "matches": ["<all_urls>"],
         "js": ["content.js"],
         "run_at": "document_idle"
       }
     ],
     "icons": {
       "16": "icons/icon16.png",
       "48": "icons/icon48.png",
       "128": "icons/icon128.png"
     }
   }
   ```

2. **Create `extension/manifest.firefox.json`:**
   Same as Chrome but with:
   ```json
   {
     "manifest_version": 3,
     "name": "odm — Open Download Manager",
     "version": "0.1.0",
     "description": "Captures browser downloads and sends them to the odm daemon for segmented, resumable downloading.",
     "permissions": [
       "downloads",
       "cookies",
       "contextMenus",
       "webRequest",
       "storage",
       "activeTab",
       "tabs"
     ],
     "host_permissions": [
       "http://127.0.0.1:*/*",
       "http://localhost:*/*",
       "<all_urls>"
     ],
     "background": {
       "scripts": ["background.js"],
       "type": "module"
     },
     "action": {
       "default_popup": "popup.html",
       "default_icon": {
         "16": "icons/icon16.png",
         "48": "icons/icon48.png",
         "128": "icons/icon128.png"
       }
     },
     "options_ui": {
       "page": "options.html",
       "open_in_tab": true
     },
     "content_scripts": [
       {
         "matches": ["<all_urls>"],
         "js": ["content.js"],
         "run_at": "document_idle"
       }
     ],
     "icons": {
       "16": "icons/icon16.png",
       "48": "icons/icon48.png",
       "128": "icons/icon128.png"
     },
     "browser_specific_settings": {
       "gecko": {
         "id": "odm@odm.dev"
       }
     }
   }
   ```

3. **Create `extension/popup.html`:**
   Self-contained popup with inline CSS. Shows:
   - Connection status indicator (green/red dot) and daemon version.
   - Detected media count for current tab with "Send All" button.
   - List of detected media URLs per tab with individual "Send" buttons.
   - Link to options page.

4. **Create `extension/popup.js`:**
   - On load: read settings from `chrome.storage.sync` (or `browser.storage.sync`).
   - Ping `http://127.0.0.1:<port>/api/v1/health` to check connectivity.
   - Query background script for detected media on current tab.
   - Render status and media list.
   - "Send" button: `POST /api/v1/downloads` with the media URL and token.

5. **Create `extension/popup.css`:**
   - Compact popup style (max-width 350px).
   - Dark and light theme via `prefers-color-scheme`.

6. **Create `extension/icons/` directory:**
   - Create placeholder SVG-based icons at 16×16, 48×48, 128×128 as simple PNG files.
   - Generate minimal icons programmatically (a simple downward arrow in a circle, exported as PNG) or use placeholder solid-color squares.
   - *Note:* For v0.1 these are placeholder icons. Proper icons are a design task.

7. **Create `extension/content.js`:**
   - Listens for messages from the background script.
   - On `"collectLinks"` message: collects all `<a href>` values from the page, filters to absolute HTTP(S) URLs, sends back to background.

8. **Commit:**
   ```powershell
   git add extension/
   git commit -m "feat(extension): create chrome and firefox manifests with shared background and popup"
   ```

---

### Task 5: Download Capture, Context Menu, and Media Detection (`extension/`) [Pending]

- **Commit Message:** `feat(extension): implement download capture, context menu, and media detection`
- **Goal:** Implement `extension/background.js` with download interception, context menu entries, and media detection via `webRequest`.

#### Steps:
1. **Implement `extension/background.js`:**
   ```javascript
   // @ts-check
   // odm browser extension — background service worker

   import { shouldIntercept } from './filter.js';

   const DEFAULT_PORT = 6820;
   const API_BASE = () => getApiBase();

   // --- State ---
   /** @type {Map<number, {url: string, mime: string}[]>} tabId → detected media */
   const detectedMedia = new Map();

   // --- Helpers ---

   async function getSettings() { /* read from chrome.storage.sync */ }
   async function getApiBase() { /* http://127.0.0.1:<port> */ }
   async function getCookiesForUrl(url) { /* chrome.cookies.getAll */ }

   async function sendToDaemon(url, extras = {}) {
     const settings = await getSettings();
     const base = `http://127.0.0.1:${settings.port || DEFAULT_PORT}`;
     const resp = await fetch(`${base}/api/v1/downloads`, {
       method: 'POST',
       headers: {
         'Content-Type': 'application/json',
         'Authorization': `Bearer ${settings.token}`,
       },
       body: JSON.stringify({ url, ...extras }),
     });
     return resp.ok;
   }

   // --- Download Capture ---
   // Chrome: chrome.downloads.onDeterminingFilename
   // Fallback: chrome.downloads.onCreated
   chrome.downloads.onCreated.addListener(async (downloadItem) => {
     const settings = await getSettings();
     if (!settings.token) return; // not configured

     const url = downloadItem.url;
     const fileSize = downloadItem.fileSize || downloadItem.totalBytes || 0;
     const hostname = new URL(downloadItem.referrer || downloadItem.url).hostname;

     const filterOpts = {
       enabled: settings.enabled !== false,
       minSizeBytes: (settings.minSize || 0) * 1024,
       extensionList: settings.extensionList
         ? settings.extensionList.split(',').map(e => e.trim()).filter(Boolean)
         : [],
       disabledSites: settings.disabledSites
         ? settings.disabledSites.split('\n').map(s => s.trim()).filter(Boolean)
         : [],
     };

     if (!shouldIntercept(url, fileSize, hostname, filterOpts)) return;

     // Try to send to daemon
     const cookies = await getCookiesForUrl(url);
     const cookieStr = cookies.map(c => `${c.name}=${c.value}`).join('; ');
     const extras = {};
     if (cookieStr) extras.options = { cookies: cookieStr };
     if (downloadItem.referrer) extras.options = { ...(extras.options || {}), referer: downloadItem.referrer };

     try {
       const ok = await sendToDaemon(url, extras);
       if (ok) {
         // Cancel the browser download
         chrome.downloads.cancel(downloadItem.id);
         chrome.downloads.erase({ id: downloadItem.id });
       }
       // If failed, let browser download proceed (fallback)
     } catch {
       // Daemon unreachable — fallback: browser download proceeds normally
     }
   });

   // --- Context Menu ---
   chrome.runtime.onInstalled.addListener(() => {
     chrome.contextMenus.create({
       id: 'odm-download-link',
       title: 'Download with odm',
       contexts: ['link'],
     });
     chrome.contextMenus.create({
       id: 'odm-download-all',
       title: 'Download all links',
       contexts: ['page'],
     });
   });

   chrome.contextMenus.onClicked.addListener(async (info, tab) => {
     if (info.menuItemId === 'odm-download-link' && info.linkUrl) {
       await sendToDaemon(info.linkUrl);
     } else if (info.menuItemId === 'odm-download-all' && tab?.id) {
       // Ask content script to collect all links
       chrome.tabs.sendMessage(tab.id, { action: 'collectLinks' }, async (links) => {
         if (chrome.runtime.lastError || !links) return;
         for (const link of links) {
           await sendToDaemon(link);
         }
       });
     }
   });

   // --- Media Detection ---
   chrome.webRequest.onBeforeRequest.addListener(
     (details) => {
       const url = details.url;
       const isMedia =
         url.includes('.m3u8') ||
         url.includes('.mpd') ||
         (details.type === 'media') ||
         (details.type === 'xmlhttprequest' && (url.includes('.m3u8') || url.includes('.mpd')));

       if (isMedia && details.tabId >= 0) {
         const tabMedia = detectedMedia.get(details.tabId) || [];
         if (!tabMedia.some(m => m.url === url)) {
           tabMedia.push({ url, mime: details.type || '' });
           detectedMedia.set(details.tabId, tabMedia);
           // Update badge
           chrome.action.setBadgeText({
             text: String(tabMedia.length),
             tabId: details.tabId,
           });
           chrome.action.setBadgeBackgroundColor({
             color: '#4a9eff',
             tabId: details.tabId,
           });
         }
       }
     },
     { urls: ['<all_urls>'] },
     []
   );

   // Clear media on tab navigation
   chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
     if (changeInfo.status === 'loading') {
       detectedMedia.delete(tabId);
       chrome.action.setBadgeText({ text: '', tabId });
     }
   });

   chrome.tabs.onRemoved.addListener((tabId) => {
     detectedMedia.delete(tabId);
   });

   // Message handler for popup to query detected media
   chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
     if (msg.action === 'getMedia' && msg.tabId != null) {
       sendResponse(detectedMedia.get(msg.tabId) || []);
     }
   });
   ```

2. **Verify:**
   - The background script is syntactically valid JavaScript modules.
   - `filter.js` import works (module type in manifest).

3. **Commit:**
   ```powershell
   git add extension/background.js extension/content.js
   git commit -m "feat(extension): implement download capture, context menu, and media detection"
   ```

---

### Task 6: Options Page for Token, Port, and Filter Configuration (`extension/`) [Pending]

- **Commit Message:** `feat(extension): implement options page for token, port, and filter configuration`
- **Goal:** Create `extension/options.html`, `extension/options.js`, and `extension/options.css` for configuring the extension: daemon token, port, minimum file size, extension filter list, and disabled sites.

#### Steps:
1. **Create `extension/options.html`:**
   - Form fields:
     - **Token:** password input with show/hide toggle.
     - **Port:** number input (default 6820).
     - **Minimum file size (KB):** number input (default 0 = no minimum).
     - **Extension filter:** textarea, comma-separated (default: `zip, exe, msi, dmg, pkg, deb, rpm, AppImage, iso, rar, 7z, tar, gz, bz2, xz, pdf, doc, docx, xls, xlsx, mp4, mkv, avi, mp3, flac, wav`).
     - **Disabled sites:** textarea, one hostname per line.
     - **Enable interception:** checkbox (default checked).
   - "Save" button, "Test Connection" button.
   - Status message area for save confirmation and connection test results.

2. **Create `extension/options.js`:**
   - On load: read settings from `chrome.storage.sync`, populate form fields.
   - On save: write settings to `chrome.storage.sync`.
   - On "Test Connection": `GET http://127.0.0.1:<port>/api/v1/health` with `Authorization: Bearer <token>`. Display success/failure.

3. **Create `extension/options.css`:**
   - Clean form layout, 600px max-width centered.
   - `prefers-color-scheme` dark/light.

4. **Commit:**
   ```powershell
   git add extension/options.html extension/options.js extension/options.css
   git commit -m "feat(extension): implement options page for token, port, and filter configuration"
   ```

---

### Task 7: Manual Checklist, Documentation and Milestone 9 Closure [Pending]

- **Commit Message:** `docs: record M9 web ui and browser extension`
- **Goal:** Document Milestone 9 features in `CHANGELOG.md` and provide a comprehensive manual verification checklist.

#### Steps:
1. **Create Manual Checklist:**
   Create `docs/manual-checklists/m9-webui-extension.md`:

   **Web UI Checklist:**
   - [ ] Start daemon, open `http://127.0.0.1:6820/` in a browser.
   - [ ] Token entry: enter correct token → main screen appears with job list.
   - [ ] Token entry: enter wrong token → error message shown.
   - [ ] "Add" button: enter a URL → download appears in job list.
   - [ ] Progress bar updates in real time via WebSocket.
   - [ ] Pause, resume, cancel buttons work per job.
   - [ ] Remove button removes the job from the list.
   - [ ] "Disconnect" clears the token and returns to the token entry screen.
   - [ ] Open on a phone browser → layout is responsive and usable.
   - [ ] WebSocket reconnects after disconnection (kill and restart daemon).
   - [ ] Speed and ETA display correctly during download.
   - [ ] Dark and light theme follows system preference.

   **Browser Extension Checklist (Chrome):**
   - [ ] Load unpacked extension from `extension/` (copy `manifest.chrome.json` to `manifest.json`).
   - [ ] Options page: configure token and port → "Test Connection" succeeds.
   - [ ] Start a download of a `.zip` file in Chrome → download is intercepted, cancelled in Chrome, and appears in odm.
   - [ ] Start a download of a `.html` file → not intercepted (not in extension list).
   - [ ] Start a download with daemon stopped → browser download proceeds normally (fallback).
   - [ ] Right-click a link → "Download with odm" → download appears in odm.
   - [ ] Right-click on a page → "Download all links" → links collected and sent to odm.
   - [ ] Visit a page with `.m3u8` request → badge shows count on toolbar icon.
   - [ ] Click toolbar popup → detected media listed with "Send" buttons.
   - [ ] Add a site to disabled list → downloads from that site are not intercepted.
   - [ ] Set minimum size → small files are not intercepted.

   **Browser Extension Checklist (Firefox):**
   - [ ] Load temporary add-on from `extension/` (rename `manifest.firefox.json` to `manifest.json`).
   - [ ] Same functional checks as Chrome above.

   **Privacy Checklist:**
   - [ ] Cookies and URLs are only sent to `127.0.0.1:<port>` (verify via network inspector).
   - [ ] No external network requests from the extension.

2. **Update Changelog:**
   Update `CHANGELOG.md` under `## [Unreleased]`:
   ```markdown
   ### Added (Milestone 9)
   - Web UI served by the daemon at `http://127.0.0.1:6820/` — job list, add URL,
     pause/resume/cancel/remove, live progress via WebSocket, responsive for phones.
   - Browser extension (Chrome and Firefox, Manifest V3) — download capture with
     fallback, context menu ("Download with odm", "Download all links"), media
     detection (.m3u8, .mpd), filter configuration (min size, extension list,
     per-site disable), and options page.
   ```

3. **Verify Full Workspace:**
   ```powershell
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo deny check
   node --test extension/test/filter.test.js
   ```

4. **Commit:**
   ```powershell
   git add CHANGELOG.md docs/manual-checklists/m9-webui-extension.md docs/superpowers/plans/2026-11-11-odm-m9-webui-extension.md
   git commit -m "docs: record M9 web ui and browser extension"
   ```
