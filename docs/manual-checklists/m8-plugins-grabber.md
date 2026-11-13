# Milestone 8: Plugins and Site Grabber Manual Verification Checklist

This document details the manual verification procedures for Milestone 8 (M8) capabilities in `odm`: Site Grabber (`odm-core::grabber`), external-process plugin host (`odm-plugin`), daemon wiring, CLI commands, and GUI screens with mandatory security disclosures.

---

## 1. Site Grabber via CLI (`odm grab`)

### 1.1 Single-Page Crawling (Depth 1)
- [ ] **CLI Execution:**
  ```bash
  odm grab https://example.com/gallery --depth 1
  ```
- [ ] **Verification:**
  - Grabber crawls `https://example.com/gallery` and outputs a formatted table containing discovered links (`<a>`, `<img>`, `<source>`, `<video>`, `<audio>`, `<link>`).
  - Verify columns: `URL`, `FILENAME`, `FOUND ON`, `DEPTH`.
  - Verify external links outside `example.com` are ignored (same-host restriction).

### 1.2 Multi-Depth Crawling (Depth > 1)
- [ ] **CLI Execution:**
  ```bash
  odm grab https://example.com/articles --depth 2 --max-pages 20
  ```
- [ ] **Verification:**
  - Pages linked from the start page on the same host are crawled up to depth 2.
  - Crawler does not exceed `max_pages` limit.
  - Discovered assets across all subpages are listed with deduplicated URLs.

### 1.3 File Extension and Pattern Filtering
- [ ] **CLI Execution:**
  ```bash
  odm grab https://example.com/docs --ext pdf zip --pattern "*manual*"
  ```
- [ ] **Verification:**
  - Only files ending in `.pdf` or `.zip` matching the pattern `*manual*` are included in the results table.
  - Extension filtering is case-insensitive (`.PDF` and `.pdf` match).

### 1.4 `robots.txt` Compliance and Bypass
- [ ] **CLI Execution with robots check:**
  ```bash
  odm grab https://example.com/site
  ```
- [ ] **Verification:**
  - Paths marked `Disallow` in `https://example.com/robots.txt` for `User-agent: *` or `User-agent: odm` are skipped.
- [ ] **CLI Execution with bypass:**
  ```bash
  odm grab https://example.com/site --no-robots
  ```
- [ ] **Verification:**
  - Disallowed paths are included in the output when `--no-robots` is passed.

### 1.5 Batch Download Enqueueing (`--add`)
- [ ] **CLI Execution:**
  ```bash
  odm grab https://example.com/downloads --ext zip --add
  ```
- [ ] **Verification:**
  - Confirmation output: `Added download #<id> (<url>)` for every matched file.
  - Run `odm list` and confirm each grabbed file is queued in `odm-daemon`.

### 1.6 Machine-Readable JSON Output
- [ ] **CLI Execution:**
  ```bash
  odm grab https://example.com/files --json
  ```
- [ ] **Verification:**
  - Valid JSON object matching `GrabberResponse` shape with `files: [...]` and `pages_crawled: N`.

---

## 2. Site Grabber via GUI

### 2.1 Opening the Grabber Window
- [ ] **Procedure:**
  - Click the `[🌐 Grabber]` button on the top toolbar.
- [ ] **Verification:**
  - Modal dialog titled `🌐 Site Grabber` opens with:
    - `Start URL` input field.
    - `Depth` slider (1 to 5).
    - `Max Pages` drag value.
    - `Extensions` text box (comma-separated).
    - `Pattern` text box.
    - `Respect robots.txt` checkbox (checked by default).
    - `🔍 Crawl` button.

### 2.2 Crawl Execution & Discovered Files Table
- [ ] **Procedure:**
  - Enter a valid URL and click `🔍 Crawl`.
- [ ] **Verification:**
  - A spinner and crawling status label appear during crawling.
  - On completion, the table populates with discovered items:
    - Checkbox column.
    - `Filename`, `Depth`, `URL`.
  - Counter displays: `(X files found across Y pages)`.
  - All discovered files are selected by default.

### 2.3 Selection & Batch Adding
- [ ] **Procedure:**
  - Click `Deselect All` -> all checkboxes uncheck.
  - Click individual file checkboxes to select desired downloads.
  - Click `➕ Add Selected (N)`.
- [ ] **Verification:**
  - Dialog closes.
  - Selected files appear in the main downloads table with active state.

---

## 3. Plugins Host & Management

### 3.1 Plugin Directory Discovery
- [ ] **Procedure:**
  - Place a test plugin folder inside `<data_dir>/plugins/test_resolver/`:
    - `plugin.toml`:
      ```toml
      [plugin]
      name = "sample-resolver"
      version = "1.0.0"
      description = "Resolves video links"
      role = "resolver"
      url_patterns = ["*://*.mysite.com/watch/*"]
      command = ["python", "resolver.py"]
      ```
    - `resolver.py`: reads JSON from stdin and prints `{"downloads": [{"url": "...", "filename": "video.mp4"}]}`.
- [ ] **Verification:**
  - Start daemon.
  - Daemon logs confirm discovery of `sample-resolver`.

### 3.2 CLI Plugin Listing & Security Notice
- [ ] **CLI Execution:**
  ```bash
  odm plugin list
  ```
- [ ] **Verification:**
  - Top of output displays the mandatory security warning:
    `Security notice: Plugins run with full user permissions without sandboxing.`
  - Table lists: `Name`, `Version`, `Role`, `URL Patterns`, `Command`.
- [ ] **JSON Output:**
  ```bash
  odm plugin list --json
  ```
  - Outputs valid JSON matching `PluginListResponse`.

### 3.3 GUI Plugin Dialog & Security Banner
- [ ] **Procedure:**
  - Click `[🔌 Plugins]` button on the top toolbar.
- [ ] **Verification:**
  - Dialog opens with a prominent high-contrast security warning box:
    `⚠️ Security Notice: Plugins are external programs running with full user permissions without sandboxing. Only install plugins from trusted sources.`
  - Grid lists all discovered plugins with details.
  - `🔄 Refresh` button re-queries daemon for newly added plugins.

---

## 4. Plugin Resolution & Backend Execution

### 4.1 URL Pattern Matching & Probe Routing
- [ ] **Procedure:**
  ```bash
  odm add "https://media.mysite.com/watch/42"
  ```
- [ ] **Verification:**
  - Daemon recognizes matching pattern and assigns `backend = "plugin"`.
  - Daemon executes the plugin child process, passing `{ "version": "0.1", "url": "..." }` on stdin.
  - Plugin returns direct download URL.
  - Download completes through HTTP backend transfer engine.

### 4.2 Timeout Isolation (30 Seconds)
- [ ] **Verification:**
  - Configure a mock plugin that sleeps indefinitely.
  - Add a matching URL.
  - Verify that after 30 seconds the child process is terminated.
  - Job fails with `PluginError::Timeout` without hanging or crashing the daemon.

### 4.3 Crash & Malformed JSON Isolation
- [ ] **Verification:**
  - Configure a plugin that exits with code 1 or emits invalid JSON.
  - Add a matching URL.
  - Daemon catches error and marks job failed with descriptive error message.
  - Daemon continues serving other requests without interruption.
