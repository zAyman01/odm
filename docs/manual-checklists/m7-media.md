# Milestone 7: Media Backends Manual Verification Checklist

This document details the manual verification procedures for Milestone 7 (M7) media backends in `odm`: HLS (`.m3u8`) streaming with AES-128 decryption, DASH (`.mpd`) streaming with audio/video muxing, yt-dlp external downloader integration, quality and format selection, ffmpeg muxing, pause/resume lifecycle across media streams, and CLI/GUI settings management.

---

## 1. HLS Backend Verification

### 1.1 CLI Probe & Master Playlist Download
- [ ] **CLI Execution:**
  ```bash
  odm add "https://example.com/stream/master.m3u8"
  ```
- [ ] **Verification:**
  - Probe detects HLS master playlist, extracts available variants/qualities (e.g., 480p, 720p, 1080p).
  - Best/first quality is selected by default.
  - Segments are fetched sequentially or concurrently, appending to `.ts` output file.
  - Speed, progress percentage, and downloaded segment counts update in real-time.

### 1.2 Quality Selection via CLI (`--quality`)
- [ ] **CLI Execution:**
  ```bash
  odm add "https://example.com/stream/master.m3u8" --quality 480p
  ```
- [ ] **Verification:**
  - Daemon resolves the specified 480p variant playlist from the master playlist.
  - Downloaded stream resolution corresponds to the 480p stream.

### 1.3 GUI Probe & Quality Dropdown
- [ ] **Procedure:**
  - Open GUI Add Dialog (`➕ Add URL`).
  - Paste an HLS master playlist URL (`.m3u8`).
  - Click `Probe` (or wait for auto-probe).
- [ ] **UI Verification:**
  - Add dialog displays a quality dropdown listing available renditions (e.g., "1920x1080 (4500 kbps)", "1280x720 (2500 kbps)", "854x480 (1200 kbps)").
  - Select a non-default quality from the dropdown.
  - Click `Download Now` and verify the selected variant is downloaded.

### 1.4 Pause and Resume Segment Progress
- [ ] **Procedure:**
  - While an HLS stream is downloading, execute:
    ```bash
    odm pause <id>
    ```
  - Verify job state becomes `Paused`. Active network requests halt.
  - Execute:
    ```bash
    odm resume <id>
    ```
- [ ] **Verification:**
  - Job resumes without re-downloading previously completed segments.
  - Saved segment progress in `HlsResumeState` is respected.

### 1.5 Download Cancellation
- [ ] **Procedure:**
  - While an HLS stream is downloading, execute:
    ```bash
    odm cancel <id>
    ```
- [ ] **Verification:**
  - Job state transitions to `Cancelled`.
  - Active segment downloads abort cleanly. Partial `.ts` file remains intact or cleans up per configuration.

### 1.6 ffmpeg Muxing (`.ts` to `.mp4`)
- [ ] **Setup:**
  - Ensure `ffmpeg` is installed and reachable via PATH or configured in settings.
- [ ] **Procedure:**
  - Download an HLS stream to completion.
- [ ] **Verification:**
  - Daemon automatically runs `ffmpeg` to mux the concatenated `.ts` file into `.mp4`.
  - Intermediate `.ts` file is deleted, and the resulting `.mp4` file plays cleanly in a media player.

### 1.7 Fallback without ffmpeg
- [ ] **Setup:**
  - Temporarily remove `ffmpeg` from PATH or set `ffmpeg_path` to an empty/invalid path.
- [ ] **Procedure:**
  - Download an HLS stream to completion.
- [ ] **Verification:**
  - Download succeeds without failing.
  - Intermediate `.ts` file is preserved as the final output file.

### 1.8 Encrypted HLS (AES-128)
- [ ] **Procedure:**
  - Add an AES-128 encrypted HLS stream (`#EXT-X-KEY:METHOD=AES-128,URI="..."`).
- [ ] **Verification:**
  - Backend fetches the encryption key from the URI.
  - Applies IV (explicit or derived from sequence number).
  - Decrypts each segment with AES-128-CBC before appending.
  - Output media plays without corruption or encryption artifacts.

### 1.9 DRM Detection & Rejection
- [ ] **Procedure:**
  - Add a DRM-protected HLS stream (`METHOD=SAMPLE-AES` or FairPlay/Widevine key formats).
- [ ] **Verification:**
  - Probe fails with a clear, user-facing error message stating DRM-protected streams are not supported.

---

## 2. DASH Backend Verification

### 2.1 CLI Probe & Download
- [ ] **CLI Execution:**
  ```bash
  odm add "https://example.com/stream/manifest.mpd"
  ```
- [ ] **Verification:**
  - Probe parses `.mpd` manifest, extracts available video and audio representations.
  - Segments for video and audio adaptation sets are downloaded.

### 2.2 GUI Probe & Quality Selection
- [ ] **Procedure:**
  - Open GUI Add Dialog and paste a DASH `.mpd` URL.
  - Observe quality dropdown populated with video representations.
  - Select desired resolution and click `Download Now`.
- [ ] **Verification:**
  - The chosen video representation is downloaded along with matching audio stream.

### 2.3 Missing ffmpeg Requirement Detection
- [ ] **Setup:**
  - Ensure `ffmpeg` is not found on PATH and `ffmpeg_path` is not set.
- [ ] **Procedure:**
  - Attempt to probe or add a DASH stream.
- [ ] **Verification:**
  - Operation fails immediately with an error explaining that `ffmpeg` is required to mux separate DASH video and audio streams.

### 2.4 Audio + Video Muxing via ffmpeg
- [ ] **Setup:**
  - Ensure `ffmpeg` is available.
- [ ] **Procedure:**
  - Download a DASH stream to completion.
- [ ] **Verification:**
  - Separate video and audio streams are muxed by `ffmpeg` into a single synchronized `.mp4` container.
  - Temporary stream files are cleaned up.

### 2.5 DRM Detection & Rejection
- [ ] **Procedure:**
  - Add a DASH stream containing `ContentProtection` elements (e.g., Widevine / PlayReady).
- [ ] **Verification:**
  - Probe detects DRM protection and fails with a clear error indicating DRM streams cannot be downloaded.

---

## 3. yt-dlp Backend Verification

### 3.1 CLI Probe & Download
- [ ] **CLI Execution:**
  ```bash
  odm add "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
  ```
- [ ] **Verification:**
  - yt-dlp is spawned with `--dump-json` to extract title, formats, and metadata.
  - Download commences with progress tracking parsed from yt-dlp stdout.
  - Job completes and produces the expected media file.

### 3.2 Format Selection via CLI (`--quality <format_id>`)
- [ ] **CLI Execution:**
  ```bash
  odm add "https://www.youtube.com/watch?v=dQw4w9WgXcQ" --quality 137+140
  ```
- [ ] **Verification:**
  - Daemon passes `-f 137+140` to yt-dlp.
  - Specified video and audio formats are downloaded and merged.

### 3.3 GUI Probe & Format Selection
- [ ] **Procedure:**
  - Open GUI Add Dialog and paste a video URL supported by yt-dlp.
  - Quality dropdown displays available formats formatted with resolution, FPS, and bitrate.
  - Select a format and submit.
- [ ] **Verification:**
  - Selected format ID is passed in `JobOptions.quality` and downloaded.

### 3.4 Pause and Resume (`--continue`)
- [ ] **Procedure:**
  - While a yt-dlp download is in progress, execute:
    ```bash
    odm pause <id>
    ```
  - Verify yt-dlp child process is terminated cleanly, state is set to `Paused`.
  - Execute:
    ```bash
    odm resume <id>
    ```
- [ ] **Verification:**
  - yt-dlp process restarts with `--continue`.
  - Progress resumes from partially downloaded media without starting from scratch.

### 3.5 Missing yt-dlp Detection & Error Guidance
- [ ] **Setup:**
  - Ensure `yt-dlp` is not in PATH and `ytdlp_path` is not configured.
- [ ] **Procedure:**
  - Attempt to add or probe a yt-dlp URL.
- [ ] **Verification:**
  - Probe fails with a clear message explaining that `yt-dlp` was not found and includes installation/configuration instructions.

### 3.6 Speed Limiting (`--speed-limit` -> `--limit-rate`)
- [ ] **CLI Execution:**
  ```bash
  odm add "<video_url>" --speed-limit 500000
  ```
- [ ] **Verification:**
  - yt-dlp process receives `--limit-rate 500000`.
  - Reported download speed does not exceed ~500 KB/s.

---

## 4. Settings Configuration (ffmpeg_path and ytdlp_path)

### 4.1 Runtime ffmpeg Path Configuration
- [ ] **CLI / API Update:**
  ```bash
  curl -X PATCH http://127.0.0.1:47321/api/v1/settings \
    -H "Content-Type: application/json" \
    -d '{"ffmpeg_path": "/custom/bin/ffmpeg"}'
  ```
- [ ] **GUI Update:**
  - Open Settings Dialog -> General tab.
  - Enter custom `ffmpeg` path in the text field and save.
- [ ] **Verification:**
  - Subsequent HLS and DASH downloads utilize the updated path.

### 4.2 Runtime yt-dlp Path Configuration
- [ ] **CLI / API Update:**
  ```bash
  curl -X PATCH http://127.0.0.1:47321/api/v1/settings \
    -H "Content-Type: application/json" \
    -d '{"ytdlp_path": "/custom/bin/yt-dlp"}'
  ```
- [ ] **GUI Update:**
  - Open Settings Dialog -> General tab.
  - Enter custom `yt-dlp` path in the text field and save.
- [ ] **Verification:**
  - Subsequent yt-dlp probes and downloads use the updated binary path.

---

## 5. Cross-Cutting & Daemon Lifecycle

### 5.1 Daemon Restart & Segment Recovery
- [ ] **Procedure:**
  - Start a multi-segment HLS or DASH download.
  - While download is active, terminate the `odm-daemon` process (SIGINT/SIGTERM or kill).
  - Restart `odm-daemon`.
- [ ] **Verification:**
  - Job state transitions back to `Paused` or resumes if configured.
  - Resuming reads `HlsResumeState` / `DashResumeState` from SQLite store and continues from already completed segments.

### 5.2 Backend Display in Job List
- [ ] **CLI Verification:**
  - Run `odm list`.
  - Check that the `Backend` column displays `hls`, `dash`, or `yt-dlp` accurately for each job.
- [ ] **GUI Verification:**
  - In GUI table and job details panel, verify `Backend: HLS`, `Backend: DASH`, or `Backend: yt-dlp` is clearly indicated.
