# Milestone 9: Web UI and Browser Extension Manual Verification Checklist

Verification checklist for odm Milestone 9 covering the embedded Web UI and Manifest V3 browser extension (Chrome & Firefox).

---

## 1. Web UI Verification (`http://127.0.0.1:6820/`)

### Setup
- [ ] Start the daemon: `cargo run -p odm-daemon`
- [ ] Open a browser and navigate to `http://127.0.0.1:6820/`

### Authentication & Connection
- [ ] Verify that on first load without saved token, the "Connect to odm" card is displayed.
- [ ] Enter an invalid token and click "Connect". Verify an error message is displayed and the main interface is not shown.
- [ ] Enter the valid daemon token (from the daemon data/config directory) and click "Connect".
- [ ] Verify token is stored in `localStorage` and the main downloads screen appears.
- [ ] Verify the header displays "odm", version tag (e.g. `v0.1.0`), a green connected indicator, "+ Add URL", and "Disconnect".
- [ ] Click "Disconnect". Verify token is cleared and the authentication screen reappears.
- [ ] Re-authenticate to continue testing.

### Downloads Management & Live Updates
- [ ] Click "+ Add URL". Verify the modal appears with URL input and focus.
- [ ] Enter a download URL (e.g., a test file or direct download link) and submit.
- [ ] Verify the modal closes and the download appears in the table.
- [ ] Verify progress bar updates dynamically via WebSocket events.
- [ ] Verify speed, percentage, and estimated time remaining (ETA) are computed and displayed.
- [ ] Test Pause: click "Pause" on an active download. Verify state badge changes to `PAUSED` and "Resume" button appears.
- [ ] Test Resume: click "Resume". Verify download resumes and state changes back to `DOWNLOADING`.
- [ ] Test Cancel: click "Cancel". Verify download enters `CANCELLED` state.
- [ ] Test Remove: click "Remove" and confirm prompt. Verify job is removed from the table.

### Responsive & Reconnection
- [ ] Open `http://127.0.0.1:6820/` on a mobile browser or emulate a mobile viewport (<650px).
- [ ] Verify table smoothly converts into responsive cards with readable labels and action buttons.
- [ ] Stop the daemon process. Verify the status indicator turns red ("Disconnected").
- [ ] Restart the daemon. Verify WebSocket reconnects automatically and the status indicator turns green.

---

## 2. Browser Extension Verification (Chrome)

### Installation
- [ ] In `extension/`, copy `manifest.chrome.json` to `manifest.json`.
- [ ] Open Chrome and navigate to `chrome://extensions/`.
- [ ] Enable "Developer mode" and click "Load unpacked". Select the `extension/` directory.
- [ ] Verify the extension loads without manifest or service worker errors.

### Options & Configuration
- [ ] Right-click the extension icon and select "Options", or click "Settings" in the popup.
- [ ] Verify the settings page opens with fields for:
  - Enable download interception (checkbox)
  - Bearer token (with Show/Hide toggle)
  - Daemon port (default 6820)
  - Minimum file size (KB)
  - Extensions list (comma-separated)
  - Disabled sites (textarea, one per line)
- [ ] Enter the bearer token and port, then click "Test Connection". Verify "Connected successfully!" message appears.
- [ ] Click "Save Settings" and verify success confirmation.

### Download Interception
- [ ] Start a download in Chrome for a matching file extension (e.g. `.zip`, `.exe`, `.pdf`).
- [ ] Verify the download is cancelled in the browser and appears in odm.
- [ ] Start a download for an unlisted file extension (e.g. `.html`). Verify it downloads normally in Chrome.
- [ ] Add a hostname to "Disabled Sites" in options and save. Start a download from that site; verify it is not intercepted.
- [ ] Set "Minimum File Size" to 1000 KB. Download a file under 1000 KB; verify it is not intercepted.
- [ ] Stop the daemon. Start a download that matches filter; verify browser fallback occurs (download completes in Chrome).

### Context Menu
- [ ] Right-click a downloadable hyperlink on any web page. Verify "Download with odm" context menu item exists.
- [ ] Click "Download with odm". Verify download is sent to odm.
- [ ] Right-click anywhere on a web page with multiple links. Click "Download all links". Verify links are collected and submitted to odm.

### Media Sniffing (.m3u8, .mpd)
- [ ] Navigate to a web page streaming video using HLS (`.m3u8`) or DASH (`.mpd`).
- [ ] Verify the extension toolbar icon shows a numeric badge with detected media count.
- [ ] Click the toolbar icon to open the popup.
- [ ] Verify detected stream URLs are listed with "Send to odm" and "Send All" buttons.
- [ ] Click "Send to odm" on a stream. Verify the media job starts in odm.

---

## 3. Browser Extension Verification (Firefox)

- [ ] In `extension/`, copy `manifest.firefox.json` to `manifest.json`.
- [ ] Open Firefox and navigate to `about:debugging#/runtime/this-firefox`.
- [ ] Click "Load Temporary Add-on..." and select `manifest.json` from `extension/`.
- [ ] Repeat options, context menu, download capture, and popup checks as in Chrome.

---

## 4. Privacy & Security Checklist

- [ ] Inspect network requests from the extension using browser DevTools / background inspect.
- [ ] Verify cookies and URLs are communicated **strictly** to `http://127.0.0.1:<port>`.
- [ ] Verify no telemetry or external network calls are made by the extension or Web UI.
