# Milestone 6: Torrent Backend Manual Verification Checklist

This document details the manual verification procedures for Milestone 6 (M6) capabilities in `odm`: BitTorrent Backend integration (`odm-torrent`) powered by `librqbit`, Magnet links, `.torrent` file downloads, selective file downloading, seeding lifecycle and ratio control, daemon persistence & restart recovery, and GUI/CLI integration.

---

## 1. BitTorrent Downloads via CLI

### 1.1 Magnet Link Download
- [ ] **CLI Execution:**
  ```bash
  odm add "magnet:?xt=urn:btih:<info_hash>&dn=Ubuntu.iso"
  ```
- [ ] **Daemon Progression:**
  - Observe job state progression: `Pending` -> `Connecting` -> `Downloading` -> `Seeding` (or `Completed` if seed ratio is 0.0).
  - Verify speed reporting (`speed_bps`), progress percentage, and downloaded bytes update in real-time.
  - Verify files are saved to the configured download directory.

### 1.2 Local .torrent File Download
- [ ] **CLI Execution:**
  ```bash
  odm add --torrent-file path/to/sample.torrent
  ```
- [ ] **Verification:**
  - Verify daemon loads and parses the metainfo without error.
  - Job downloads data pieces from peers/seeders.
  - Output files are assembled and match original torrent contents.

### 1.3 Selective File Downloading
- [ ] **CLI Execution:**
  ```bash
  odm add --torrent-file path/to/multi_file.torrent --select-files 0,2
  ```
- [ ] **Verification:**
  - In multi-file torrent, only files at index 0 and 2 are downloaded to disk.
  - Deselected files are skipped or marked unselected in `librqbit`.

---

## 2. BitTorrent Downloads via GUI

### 2.1 Add Dialog Probe & File Selection
- [ ] **Procedure:**
  - Open GUI Add Dialog (`➕ Add URL`).
  - Paste a magnet URI or local path to a `.torrent` file.
  - Click `Probe` (or allow auto-probe).
- [ ] **UI Verification:**
  - The dialog displays the detected filename, total size, and a scrollable `Torrent Files` checklist.
  - All files are checked by default.
  - Uncheck one or more files and submit with `Download Now`.
  - Verify only checked files are retrieved.

### 2.2 Sidebar "Torrents" Filter
- [ ] **Sidebar Verification:**
  - Click `🧲 Torrents` in the left sidebar.
  - Verify only jobs with `backend == "torrent"` are shown.
  - Verify the counter reflects the exact number of active/completed torrent downloads.

### 2.3 Job Detail Panel & Seeding Display
- [ ] **Detail Panel Verification:**
  - Select a torrent download from the table.
  - Observe bottom panel displaying `Backend: Torrent | State: <state>`.
  - When job is seeding, target seed ratio is displayed.

---

## 3. Seeding Lifecycle & Ratio Controls

### 3.1 Immediate Completion (Seed Ratio = 0.0)
- [ ] **CLI Execution:**
  ```bash
  odm add "magnet:..." --seed-ratio 0.0
  ```
- [ ] **Verification:**
  - As soon as 100% of data is downloaded and verified, the job transitions directly to `Completed` without lingering in `Seeding`.

### 3.2 Target Seeding (Seed Ratio = 2.0)
- [ ] **CLI Execution:**
  ```bash
  odm add "magnet:..." --seed-ratio 2.0
  ```
- [ ] **Verification:**
  - Once download reaches 100%, job transitions to `Seeding`.
  - Torrent session continues uploading until uploaded bytes / downloaded bytes >= 2.0.
  - Upon reaching ratio 2.0, state automatically transitions to `Completed`.

---

## 4. Pause, Resume, and Cancel Lifecycle

### 4.1 Pausing and Resuming
- [ ] **Procedure:**
  - While torrent is downloading or seeding, execute:
    ```bash
    odm pause <id>
    ```
  - Verify job state becomes `Paused`. Active peer network traffic stops.
  - Execute:
    ```bash
    odm resume <id>
    ```
  - Verify job transitions back to `Downloading`/`Seeding` and continues without data loss or re-downloading finished pieces.

### 4.2 Cancellation
- [ ] **Procedure:**
  - Execute `odm cancel <id>`.
  - Verify state transitions to `Cancelled`.
  - Torrent is cleanly stopped and removed from `librqbit::Session`.

---

## 5. Daemon Restart & Crash Recovery

### 5.1 Recovery from Restart
- [ ] **Procedure:**
  - Start a torrent download with multiple pieces.
  - Abruptly stop or kill the `odm-daemon` process while downloading.
  - Restart `odm-daemon`.
- [ ] **Verification:**
  - Daemon reads `backend_state` from SQLite store (`TorrentResumeState`).
  - Torrent backend re-initializes session and restores the torrent without corruption.
  - Download resumes from the last saved state.
