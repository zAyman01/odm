# Milestone 4: GUI Desktop Client Manual Verification Checklist

This document details the manual verification procedures for the `odm-gui` desktop application. Follow each section in order on Windows, Linux, or macOS to verify end-to-end user experience and graphical functionality.

---

## 1. Application Launch & Window Configuration

- [ ] **Launch:** Run `cargo run -p odm-gui` (or the compiled binary `odm-gui.exe`).
- [ ] **Window Dimensions:** Verify the application window opens with default dimensions (1024x640), respects minimum resize bounds (720x480), and features the title: `odm - Open Download Manager`.
- [ ] **Theme Default:** Verify that dark theme is applied cleanly without font rendering glitches or clipped widgets.

---

## 2. Daemon Connection Status

- [ ] **Offline Indicator:** When `odm-daemon` is not running, the top-right toolbar indicator shows `○ Daemon Offline` in red.
- [ ] **Online Indicator:** Start the daemon (`cargo run -p odm-daemon`). The indicator changes to `● Daemon Connected` in green within 2 seconds.
- [ ] **Reconnection:** Stop the daemon. Observe the status transitions to offline. Restart the daemon. Observe automatic background reconnection and state resync.

---

## 3. Top Toolbar Controls

- [ ] **Add Download:** Click `➕ Add URL`. Verify the modal "Add Download" dialog opens.
- [ ] **Action Button States:**
  - With no download selected in the table, verify `▶ Resume`, `⏸ Pause`, and `🗑 Remove` buttons are disabled.
  - When a download row is clicked, verify these buttons become enabled.
- [ ] **Theme Toggle:** Click `☀ Light / 🌙 Dark`. Verify the entire interface immediately switches between dark and light palettes.
- [ ] **Settings Button:** Click `⚙ Settings`. Verify the settings modal opens.

---

## 4. Left Sidebar Filters & Queues

- [ ] **Filter Selection:**
  - Click `📁 All`: shows all downloads.
  - Click `⚡ Active`: shows only downloading and pending downloads.
  - Click `✓ Completed`: shows only completed downloads.
- [ ] **Badge Counts:** Verify that the counts in parentheses accurately reflect the number of matching items.
- [ ] **Queues:** Create a custom queue or click `🗂 default`. Verify downloads in other queues are excluded.
- [ ] **Categories:** Click `🏷 Documents`, `🏷 Archives`, `🏷 Media`. Verify file extension-based categorization.

---

## 5. Downloads Table & Visual Progress

- [ ] **Grid Layout:** Verify columns: `ID`, `Filename`, `Size`, `Progress`, `Speed`, and `Status`.
- [ ] **Progress Bars:** Observe smooth bar filling (`egui::ProgressBar`) with percentage label (e.g. `45.2%`).
- [ ] **Speed Updates:** Verify transfer speed is dynamically formatted (e.g., `1.5 MB/s`).
- [ ] **Status Colorization:**
  - `downloading`: Blue
  - `completed`: Green
  - `paused`: Yellow
  - `failed`: Red

---

## 6. Bottom Detail Panel & Segment Visualizer

- [ ] **Row Selection:** Click a download row. Verify the bottom panel slides up or displays details:
  - Download ID and Filename heading
  - Target directory
  - Queue assignment
- [ ] **Segment Chunk Visualizer:**
  - Verify horizontal bar divided into multi-connection segment blocks.
  - Green blocks represent finished segments (`seg.is_complete`).
  - Blue blocks represent active downloading segments (`seg.downloaded > 0`).
  - Gray blocks represent pending ranges.
- [ ] **Error Display:** For a failed job, verify error details appear in bold red.

---

## 7. Add Download Dialog & Server Probing

- [ ] **URL Input:** Paste a test URL (e.g. `http://speedtest.tele2.net/10MB.zip`).
- [ ] **Probe Server:** Click `Probe`. Verify the spinner appears and server metadata (filename, size, range capability) is populated.
- [ ] **Custom Settings:**
  - Change target filename.
  - Adjust segment count slider (e.g., 8 segments).
  - Adjust speed limit.
  - Choose queue from dropdown.
- [ ] **Download Now:** Click `Download Now`. Verify dialog closes and the item appears in the main table.

---

## 8. Settings Dialog

- [ ] **Default Directory:** Change default download directory.
- [ ] **Default Segments:** Adjust slider (1..32).
- [ ] **Global Speed Limit:** Set limit or slide to 0 for unlimited.
- [ ] **Clipboard Monitoring Toggle:** Enable/disable clipboard monitoring checkbox.
- [ ] **Save & Apply:** Click `Save`. Verify preferences persist and update runtime behavior.

---

## 9. Clipboard URL Monitoring

- [ ] **Copy Detection:** With the GUI running in the background, copy an HTTP/HTTPS URL from a browser or text editor to the OS clipboard.
- [ ] **Dialog Trigger:** Verify the "Add Download" modal opens automatically with the copied URL prefilled.
- [ ] **Non-Download Filter:** Copy non-URL text or local file paths (`file:///`). Verify the dialog is not triggered.
- [ ] **Duplicate Prevention:** Copy the same URL again. Verify duplicate popups are suppressed.

---

## 10. Desktop OS Notifications

- [ ] **Completion Notification:** Allow a test download to reach 100%. Verify an OS desktop notification appears (`Download Completed: <filename>`).
- [ ] **Failure Notification:** Trigger a download error (e.g., disconnecting network or unreachable server). Verify an error notification appears.
