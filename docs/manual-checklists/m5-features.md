# Milestone 5: Features Manual Verification Checklist

This document details the manual verification procedures for Milestone 5 (M5) capabilities in `odm`: Cryptographic Checksum Verification, Multi-Mirror Failover, Metalink Import, Batch URL Pattern Expansion, File Categories & Auto-Routing, and Queue Scheduling.

---

## 1. Cryptographic Checksum Verification

### 1.1 Matching Checksum (Pass)
- [ ] **CLI Execution:**
  ```bash
  odm add http://localhost:8080/testfile.bin --checksum sha256:<valid-64-char-hex>
  ```
- [ ] **Daemon Progression:**
  - Observe job transitions: `Pending` -> `Connecting` -> `Downloading` -> `Verifying` -> `Completed`.
  - In `Verifying` state, check that streaming hash digest calculation runs across the assembled file without re-reading the entire file into memory at once.
- [ ] **GUI Add Dialog:**
  - Open Add Dialog (`➕ Add URL`).
  - Select checksum algorithm (`SHA-256`, `SHA-1`, or `MD5`).
  - Paste expected digest into hash input field.
  - Submit download and verify table shows completed status upon successful hash match.

### 1.2 Mismatched Checksum (Integrity Failure)
- [ ] **CLI Execution:**
  ```bash
  odm add http://localhost:8080/corrupt.bin --checksum sha256:0000000000000000000000000000000000000000000000000000000000000000
  ```
- [ ] **Failure Verification:**
  - Verify job enters `Failed` state.
  - Verify error message indicates `checksum_mismatch: expected 0000..., computed <actual>`.
  - In GUI, verify the failed row is displayed in red and the bottom details panel shows the checksum mismatch error.

---

## 2. Multi-Mirror Downloads & Automatic Failover

### 2.1 Segment Distribution Across Mirrors
- [ ] **CLI Execution:**
  ```bash
  odm add http://primary.example.com/data.iso --mirror http://mirror1.example.com/data.iso --mirror http://mirror2.example.com/data.iso --segments 6
  ```
- [ ] **Distribution Check:**
  - In GUI segment visualizer or daemon logs, verify segments 0..5 connect to available hosts round-robin or based on availability.

### 2.2 Primary Failure & Mirror Failover
- [ ] **Setup:**
  - Provide an invalid/failing primary URL (e.g. `http://127.0.0.1:9999/bad`) with a working mirror (`http://127.0.0.1:8080/good`).
- [ ] **Verification:**
  - Segment connection attempt to primary fails with connection refusal or timeout.
  - Engine automatically retries range request against the secondary mirror.
  - Job completes successfully without user intervention.

---

## 3. Metalink (.metalink / .meta4) Import

### 3.1 CLI Metalink Parsing
- [ ] **CLI Command:**
  ```bash
  odm add --metalink path/to/sample.meta4
  ```
- [ ] **Verification:**
  - CLI parses XML document conformant to RFC 5854 (Metalink 4) and Metalink 3.
  - Verifies extracted target filename, primary URLs, mirrors, and SHA-256 / SHA-1 hashes.
  - Creates the job with pre-attached mirrors and checksum options.

---

## 4. Batch URL Pattern Expansion

### 4.1 Numerical Range with Zero-Padding
- [ ] **CLI Command:**
  ```bash
  odm add "http://example.com/images/img-[001-010].jpg"
  ```
- [ ] **Verification:**
  - Verify 10 distinct download jobs are submitted: `img-001.jpg` through `img-010.jpg`.
  - Check that leading zeroes are preserved as requested by the padding format.

### 4.2 Stepped & Multi-Bracket Cartesian Expansion
- [ ] **CLI Command:**
  ```bash
  odm add "http://example.com/data-[a-b]-[1-2].bin"
  ```
- [ ] **Verification:**
  - Verify 4 jobs are generated: `data-a-1.bin`, `data-a-2.bin`, `data-b-1.bin`, `data-b-2.bin`.

### 4.3 GUI Batch Preview
- [ ] **GUI Input:**
  - Open Add Dialog (`➕ Add URL`).
  - Paste `http://example.com/part-[1-5].rar`.
  - Verify the blue preview badge appears immediately: `Batch URL pattern: 5 URLs will be added`.
  - Click `Download Now` and verify all 5 download entries appear in the main downloads table.

---

## 5. File Categories & Automatic Routing

### 5.1 Default Category Auto-Routing
- [ ] **File Extension Matching:**
  - Add `http://example.com/test.zip` without specifying target directory.
  - Verify `test.zip` is automatically routed to the `compressed` category directory.
  - Add `http://example.com/manual.pdf`. Verify routing to `documents`.
  - Add `http://example.com/song.mp3`. Verify routing to `music`.
  - Add `http://example.com/video.mp4`. Verify routing to `video`.
  - Add `http://example.com/installer.exe`. Verify routing to `programs`.

### 5.2 Category CRUD via CLI
- [ ] **Commands:**
  ```bash
  odm category list
  odm category set custom --dir "C:\Downloads\Custom" --exts "xyz,abc"
  odm category list
  odm category delete custom
  ```
- [ ] **Verification:**
  - `list` returns formatted table of category name, target directory, and associated extensions.
  - `set` creates or updates category directory and extension mappings.
  - `delete` successfully removes custom category.

### 5.3 GUI Category Management & Sidebar Badges
- [ ] **Settings Modal > Categories Tab:**
  - Open `Settings` -> select `Categories` tab.
  - Modify target directory for `compressed` to a custom path and click `Save`.
  - Verify updated path persists across restarts.
- [ ] **Sidebar Category Filtering:**
  - In left navigation sidebar, observe dynamic categories (`compressed`, `documents`, `music`, etc.).
  - Observe count badges next to category names (e.g. `🏷 compressed (3)`).
  - Click a category. Verify table filters downloads to only matching items.

---

## 6. Queue Scheduling & Background Schedule Manager

### 6.1 CLI Queue Schedule Configuration
- [ ] **CLI Command:**
  ```bash
  odm queue schedule nightly --start 02:00 --stop 06:00 --days mon,tue,wed,thu,fri
  ```
- [ ] **Verification:**
  - Query queue details: verify `schedule_start = "02:00"`, `schedule_stop = "06:00"`, `schedule_days = ["mon", "tue", "wed", "thu", "fri"]`.

### 6.2 GUI Settings > Scheduler Tab
- [ ] **Configuration:**
  - Open `Settings` -> select `Scheduler` tab.
  - Select queue from dropdown.
  - Input Start time `23:30`, Stop time `06:00`, and Days `sat,sun`.
  - Click `Apply Schedule`.
  - Reopen dialog and verify scheduled values are preserved.

### 6.3 Daemon Scheduler Trigger Execution
- [ ] **Auto-Start at Start Time:**
  - Configure queue schedule with start time matching current local minute (`HH:MM`).
  - Queue has paused/pending downloads.
  - Observe scheduler background worker ticks every 30 seconds.
  - At matching minute, verify queue automatically starts (`run_queue`) and queued jobs transition to `downloading`.
- [ ] **Auto-Pause at Stop Time:**
  - Set stop time to the next minute.
  - When the clock ticks to stop time, verify all active jobs in the queue are gracefully paused.
