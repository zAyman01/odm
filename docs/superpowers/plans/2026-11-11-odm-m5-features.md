# Milestone 5: Checksums, Mirrors, Metalink, Categories, Scheduler, and Batch URLs Implementation Plan

> **Specification Reference:** `docs/superpowers/specs/2026-11-11-odm-design.md` (Sections 3, 5, 6, 12).  
> **Milestone Success Criterion:** Each feature usable from CLI and GUI (verified by automated tests across `odm-core`, `odm-daemon`, `odm-cli`, and `odm-gui`, plus manual checklist `docs/manual-checklists/m5-features.md`).

---

## Architecture and Key Design Decisions

1. **Checksum Verification & Streaming Validation (`odm-core`, `odm-api`)**:
   - `ChecksumAlgorithm`: `Sha256`, `Sha1`, `Md5`.
   - `Checksum`: `{ algorithm: ChecksumAlgorithm, digest: String }`. String parsing format: `"sha256:<hex>"`, `"sha1:<hex>"`, `"md5:<hex>"`.
   - Streaming hash calculation: when a job with a checksum finishes transferring all bytes, it enters `JobState::Verifying`. The target file is hashed in a single streaming pass without loading into memory. If the hash matches, it transitions to `Completed`; on mismatch, it transitions to `Failed` with a detailed error (`checksum mismatch: expected ... got ...`, `retryable = false`).

2. **Batch URL Expansion (`odm-core::batch`)**:
   - Pure syntactic parser expanding bracketed ranges in URLs:
     - Numerical ranges: e.g. `http://example.com/file[001-100].zip` -> 100 URLs with 3-digit zero-padding preserved. Optional step e.g. `[1-10:2]`.
     - Alphabetic ranges: e.g. `http://example.com/part_[a-z].tar` -> 26 URLs.
     - Cartesian products: multiple bracket sets in a single URL (e.g. `[01-02]_[a-b]`).
     - Normal URLs without brackets return a single-element list.

3. **Mirrors & Metalink (`odm-core`)**:
   - `JobOptions` and `JobOptionsDto` support `mirrors: Vec<String>`.
   - HTTP Segmented Downloader spreads initial segments and dynamic segments across the primary URL and available mirrors.
   - If a mirror repeatedly encounters network errors or HTTP 5xx responses exceeding segment retries, that mirror is dropped from the active mirror pool and its assigned segments fail over to remaining mirrors.
   - Metalink Parser (`odm-core::metalink`): Uses `quick-xml` to parse `.metalink` and `.meta4` XML documents into filename, size, ordered mirror URLs (sorted by priority), and expected cryptographic checksums.

4. **Categories & Auto-Routing (`odm-core`, `odm-api`, `odm-daemon`)**:
   - Default categories:
     - `Compressed`: `zip, rar, 7z, tar, gz, bz2, xz`
     - `Documents`: `pdf, doc, docx, txt, rtf, odt, epub`
     - `Music`: `mp3, flac, wav, m4a, aac, ogg`
     - `Video`: `mp4, mkv, avi, mov, wmv, webm`
     - `Programs`: `exe, msi, dmg, pkg, deb, rpm, appimage, iso`
     - `Other`: catch-all
   - SQLite migration 4 introduces `categories` table (`name`, `directory`, `extensions_json`).
   - If `target_dir` is not explicitly supplied when adding a job, the core auto-detects the matching category based on the filename extension and assigns the category's download directory.
   - REST API: `GET /api/v1/categories`, `PUT /api/v1/categories/{name}`, `DELETE /api/v1/categories/{name}`.

5. **Queue Schedules & Scheduler (`odm-core`, `odm-api`, `odm-daemon`)**:
   - Schedule schema: `start_time: Option<String>` (`"HH:MM"`), `stop_time: Option<String>` (`"HH:MM"`), `days: Vec<String>` (`["mon", "tue", ...]`).
   - Added to `queues` table in SQLite migration 4.
   - In `odm-daemon`, a background periodic scheduler task evaluates queue schedules once every minute against the current local time:
     - If current time reaches `start_time` on an active day: auto-starts queued downloads for that queue (`engine.run_queue(&queue.name)`).
     - If current time reaches `stop_time` on an active day: pauses active jobs belonging to that queue.
     - Injectable clock / time provider for deterministic unit and integration testing.

6. **CLI & GUI Interface Exposure (`odm-cli`, `odm-gui`)**:
   - CLI:
     - `odm add <url>...` auto-expands batch URLs, accepts `--checksum <algo:hash>`, `--mirror <url>...`, `--metalink <file>`, and `--category <name>`.
     - `odm category list`, `odm category set <name> --dir <dir> --exts <csv>`, `odm category delete <name>`.
     - `odm queue schedule <name> --start <HH:MM> [--stop <HH:MM>] [--days <csv>]`.
   - GUI:
     - Add Dialog: checksum input field, mirrors list input, live batch pattern preview, metalink loading.
     - Sidebar: dynamic category list synced with daemon.
     - Settings Dialog: category directory configuration and queue scheduler settings.

---

## File Structure Plan

```
crates/odm-api/
  src/
    category.rs          # CategoryDto, SetCategoryRequest, ListCategoriesResponse
    checksum.rs          # ChecksumAlgorithm, ChecksumDto
    download.rs          # Updated JobOptionsDto with checksum, mirrors, category
    queue.rs             # Updated QueueDto, CreateQueueRequest, UpdateQueueRequest with schedule
crates/odm-core/
  src/
    batch.rs             # Batch URL bracket range parser and expander
    checksum.rs          # Streaming SHA-256, SHA-1, MD5 file validator
    metalink.rs          # Quick-xml Metalink 3 / Meta4 parser
    category.rs          # Category model and extension matcher
    schedule.rs          # QueueSchedule model and time comparison logic
    store.rs             # Migration 4: categories table, queue schedule columns
    http.rs              # Mirror pool rotation and segment failover
    engine.rs            # Verifying state transition and checksum verification pass
crates/odm-daemon/
  src/
    scheduler.rs         # Background schedule evaluator
    routes/
      categories.rs      # GET, PUT, DELETE /api/v1/categories
      queues.rs          # Updated with schedule fields
crates/odm-cli/
  src/
    commands/
      category.rs        # odm category list|set|delete
      queue.rs           # odm queue schedule
      add.rs             # Updated with batch expansion, --checksum, --mirror, --metalink
crates/odm-gui/
  src/
    ui/
      add_dialog.rs      # Updated with checksum, mirrors, batch preview
      settings_dialog.rs # Category directory configuration, queue schedule configuration
docs/manual-checklists/
  m5-features.md         # Interactive tester checklist for M5 features
CHANGELOG.md             # Updated with M5 release notes
```

---

### Task 1: Checksum verification and hashing pass (`odm-core`, `odm-api`)

**Files:**
- Modify: `crates/odm-api/Cargo.toml`
- Create: `crates/odm-api/src/checksum.rs`
- Modify: `crates/odm-api/src/download.rs`
- Modify: `crates/odm-api/src/lib.rs`
- Modify: `crates/odm-core/Cargo.toml`
- Create: `crates/odm-core/src/checksum.rs`
- Modify: `crates/odm-core/src/job.rs`
- Modify: `crates/odm-core/src/engine.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Create: `crates/odm-core/tests/checksum_verification.rs`

**Interfaces:**
- Consumes: `sha2 = "0.10.8"`, `sha1 = "0.10.6"`, `md-5 = "0.10.6"`.
- Produces: `ChecksumAlgorithm`, `ChecksumDto`, `Checksum`, `verify_file_checksum`, streaming verification pass in `Engine::run` leading to `JobState::Verifying -> Completed | Failed`.

- [x] **Step 1: Write checksum verification unit test first**

`crates/odm-core/tests/checksum_verification.rs`:
- Tests `verify_file_checksum` against SHA-256, SHA-1, and MD5 for matching digests.
- Tests `verify_file_checksum` reporting mismatch error when digest differs.
- Tests `Engine::run` transitioning through `JobState::Verifying` and failing on corrupted download or succeeding on valid download.

- [x] **Step 2: Add dependencies in `Cargo.toml`**

Add `sha2 = "0.10.8"`, `sha1 = "0.10.6"`, and `md-5 = "0.10.6"` to `crates/odm-core/Cargo.toml`.
Run `cargo deny check`.

- [x] **Step 3: Implement Checksum models and verification logic**

In `crates/odm-api/src/checksum.rs`:
- Define `ChecksumAlgorithm { Sha256, Sha1, Md5 }`.
- Define `ChecksumDto { algorithm: ChecksumAlgorithm, digest: String }`.
- Implement `FromStr` parsing `"sha256:<digest>"`, `"sha1:<digest>"`, `"md5:<digest>"`.

In `crates/odm-core/src/checksum.rs`:
- Implement streaming verification `verify_file_checksum(path: &Path, checksum: &Checksum) -> Result<(), ChecksumError>`.

In `crates/odm-core/src/engine.rs`:
- In `Engine::run`, when `outcome == RunOutcome::Completed`:
  - If `job.options().checksum` is `Some(checksum)`:
    - Transition to `JobState::Verifying` and persist + broadcast event.
    - Run streaming file hash verification.
    - If valid: transition to `JobState::Completed`.
    - If mismatch: set error and transition to `JobState::Failed`.

- [x] **Step 4: Run checksum unit tests and verify gate**

Run:
```bash
cargo test -p odm-core --test checksum_verification
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [x] **Step 5: Commit**

```bash
git add crates/odm-api crates/odm-core Cargo.lock
git commit -m "feat(core): implement checksum verification and verifying state pass"
```

---

### Task 2: Batch URL pattern expansion (`odm-core`, `odm-api`)

**Files:**
- Create: `crates/odm-core/src/batch.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Create: `crates/odm-core/tests/batch_urls.rs`

**Interfaces:**
- Produces: `expand_batch_url(pattern: &str) -> Result<Vec<String>, BatchError>`.

- [ ] **Step 1: Write batch URL expansion unit tests first**

`crates/odm-core/tests/batch_urls.rs`:
- Test numerical ranges: `http://example.com/file[001-005].zip` expands to `file001.zip` through `file005.zip`.
- Test reverse ranges / step: `[1-9:2]` expands to `1, 3, 5, 7, 9`.
- Test character ranges: `[a-c]` expands to `a, b, c`.
- Test cartesian product: `[01-02]_[a-b]` expands to 4 URLs.
- Test normal URL with no brackets returns single URL.
- Test syntax errors (unclosed brackets, invalid numbers) return typed error.

- [ ] **Step 2: Implement batch expansion parser**

In `crates/odm-core/src/batch.rs`:
- Implement `expand_batch_url` using recursive / Cartesian product expansion.
- Preserve zero-padding width based on starting number string length.

- [ ] **Step 3: Run batch tests and verify gate**

Run:
```bash
cargo test -p odm-core --test batch_urls
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 4: Commit**

```bash
git add crates/odm-core
git commit -m "feat(core): implement batch url bracket pattern expansion"
```

---

### Task 3: Metalink parsing and mirror failover (`odm-core`, `odm-api`)

**Files:**
- Modify: `crates/odm-core/Cargo.toml`
- Create: `crates/odm-core/src/metalink.rs`
- Modify: `crates/odm-core/src/http.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Modify: `crates/odm-api/src/download.rs`
- Create: `crates/odm-core/tests/metalink_and_mirrors.rs`

**Interfaces:**
- Consumes: `quick-xml = "0.41.0"`.
- Produces: `parse_metalink(xml: &str) -> Result<MetalinkSpec, MetalinkError>`, mirror rotation in `HttpBackend`.

- [ ] **Step 1: Write metalink & mirror tests first**

`crates/odm-core/tests/metalink_and_mirrors.rs`:
- Test parsing valid `.metalink` (v3) XML with `<file>`, `<url>`, `<verification><hash>`.
- Test parsing valid `.meta4` (v4) XML with `<file>`, `<url priority="...">`, `<hash type="sha-256">`.
- Test multi-mirror HTTP download: segments distributed between mock servers; if one mock server returns 500, remaining server handles the segment.

- [ ] **Step 2: Add quick-xml to `crates/odm-core/Cargo.toml`**

Add `quick-xml = "0.41.0"`. Run `cargo deny check`.

- [ ] **Step 3: Implement Metalink parser and mirror pool**

In `crates/odm-core/src/metalink.rs`:
- Parse XML using `quick_xml::Reader`.
- Extract filename, size, mirror URLs ordered by priority/preference, and checksums.

In `crates/odm-core/src/http.rs`:
- Support `mirrors: Vec<String>` in `JobOptions`.
- Create a `MirrorPool` managing available URLs.
- Distribute segment requests across mirrors.
- On segment failure, mark failing mirror degraded and reassign segment to an active mirror.

- [ ] **Step 4: Run metalink & mirror tests and verify gate**

Run:
```bash
cargo test -p odm-core --test metalink_and_mirrors
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 5: Commit**

```bash
git add crates/odm-core crates/odm-api Cargo.lock
git commit -m "feat(core): implement metalink parsing and multi-mirror failover"
```

---

### Task 4: Categories: model, persistence, auto-routing and API (`odm-core`, `odm-api`, `odm-daemon`)

**Files:**
- Create: `crates/odm-api/src/category.rs`
- Modify: `crates/odm-api/src/lib.rs`
- Create: `crates/odm-core/src/category.rs`
- Modify: `crates/odm-core/src/store.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Create: `crates/odm-daemon/src/routes/categories.rs`
- Modify: `crates/odm-daemon/src/routes/mod.rs`
- Modify: `crates/odm-daemon/src/server.rs`
- Create: `crates/odm-core/tests/categories.rs`
- Create: `crates/odm-daemon/tests/categories_api.rs`

**Interfaces:**
- Produces: `CategoryDto`, `SetCategoryRequest`, `ListCategoriesResponse`, SQLite migration 4 (`categories`), REST routes `/api/v1/categories`.

- [ ] **Step 1: Write category persistence and API tests first**

`crates/odm-core/tests/categories.rs`:
- Verifies migration 4 seeds default categories.
- Verifies `category_for_filename("archive.tar.gz")` returns `Compressed`.
- Verifies creating, updating, and deleting categories.
- Verifies job created without `target_dir` routes into the category directory.

`crates/odm-daemon/tests/categories_api.rs`:
- Verifies `GET /api/v1/categories` returns list.
- Verifies `PUT /api/v1/categories/{name}` updates or creates.
- Verifies `DELETE /api/v1/categories/{name}` deletes non-default.

- [ ] **Step 2: Implement category models and migration 4**

In `crates/odm-api/src/category.rs`:
- Define `CategoryDto`, `SetCategoryRequest`, `ListCategoriesResponse`.

In `crates/odm-core/src/store.rs`:
- Add `MIGRATION_4`:
  ```sql
  CREATE TABLE categories (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      name TEXT NOT NULL UNIQUE,
      directory TEXT NOT NULL,
      extensions_json TEXT NOT NULL
  );
  ALTER TABLE jobs ADD COLUMN category TEXT;
  ```
- Seed default categories.
- Implement Store methods: `list_categories`, `get_category`, `set_category`, `delete_category`, `category_for_filename`.

In `crates/odm-daemon/src/routes/categories.rs`:
- Implement Axum handlers: `list_categories`, `set_category`, `delete_category`.

- [ ] **Step 3: Run category tests and verify gate**

Run:
```bash
cargo test -p odm-core --test categories
cargo test -p odm-daemon --test categories_api
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 4: Commit**

```bash
git add crates/odm-api crates/odm-core crates/odm-daemon
git commit -m "feat(daemon): implement categories persistence, auto-routing and rest api"
```

---

### Task 5: Queue schedules and background scheduler (`odm-core`, `odm-api`, `odm-daemon`)

**Files:**
- Modify: `crates/odm-api/src/queue.rs`
- Create: `crates/odm-core/src/schedule.rs`
- Modify: `crates/odm-core/src/store.rs`
- Modify: `crates/odm-core/src/lib.rs`
- Create: `crates/odm-daemon/src/scheduler.rs`
- Modify: `crates/odm-daemon/src/lib.rs`
- Modify: `crates/odm-daemon/src/server.rs`
- Modify: `crates/odm-daemon/src/routes/queues.rs`
- Create: `crates/odm-daemon/tests/queue_scheduler.rs`

**Interfaces:**
- Produces: `QueueScheduleDto`, `QueueSchedule`, background `Scheduler` worker in `odm-daemon`.

- [ ] **Step 1: Write scheduler tests first**

`crates/odm-daemon/tests/queue_scheduler.rs`:
- Test schedule evaluation: matches start time and active weekday.
- Test queue auto-start when clock hits start time.
- Test queue jobs auto-pause when clock hits stop time.

- [ ] **Step 2: Implement schedule schema and models**

In `crates/odm-api/src/queue.rs`:
- Extend `QueueDto`, `CreateQueueRequest`, `UpdateQueueRequest` with `schedule_start`, `schedule_stop`, `schedule_days`.

In `crates/odm-core/src/store.rs`:
- In `MIGRATION_4` (or `MIGRATION_5`):
  ```sql
  ALTER TABLE queues ADD COLUMN schedule_start TEXT;
  ALTER TABLE queues ADD COLUMN schedule_stop TEXT;
  ALTER TABLE queues ADD COLUMN schedule_days TEXT;
  ```
- Persist and retrieve queue schedule fields.

In `crates/odm-daemon/src/scheduler.rs`:
- Implement `Scheduler`:
  - Periodically checks all queues.
  - Matches local time `HH:MM` against `schedule_start` / `schedule_stop`.
  - Dispatches `engine.run_queue` or pauses active jobs.

- [ ] **Step 3: Run scheduler tests and verify gate**

Run:
```bash
cargo test -p odm-daemon --test queue_scheduler
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 4: Commit**

```bash
git add crates/odm-api crates/odm-core crates/odm-daemon
git commit -m "feat(daemon): implement queue scheduling and background schedule manager"
```

---

### Task 6: CLI integration for M5 features (`odm-cli`)

**Files:**
- Modify: `crates/odm-cli/Cargo.toml`
- Modify: `crates/odm-cli/src/main.rs`
- Create: `crates/odm-cli/src/commands/category.rs`
- Modify: `crates/odm-cli/src/commands/queue.rs`
- Modify: `crates/odm-cli/src/commands/add.rs`
- Modify: `crates/odm-cli/src/commands/mod.rs`
- Create: `crates/odm-cli/tests/m5_cli_features.rs`

**Interfaces:**
- Produces:
  - `odm add <url>... --checksum <algo:hash> --mirror <url>... --metalink <file> --category <name>`
  - `odm category list|set|delete`
  - `odm queue schedule <name> --start <HH:MM> --stop <HH:MM> --days <days>`

- [ ] **Step 1: Write CLI M5 integration test first**

`crates/odm-cli/tests/m5_cli_features.rs`:
- Test CLI `odm add` with batch bracket URL: expands and submits multiple jobs.
- Test CLI `odm add` with `--checksum sha256:...`.
- Test CLI `odm category list` and `odm category set`.
- Test CLI `odm queue schedule`.

- [ ] **Step 2: Implement CLI commands and flag parsing**

In `crates/odm-cli/src/commands/add.rs`:
- Expand batch patterns with `odm_core::batch::expand_batch_url`.
- Support `--checksum`, `--mirror`, `--metalink`, `--category`.

In `crates/odm-cli/src/commands/category.rs`:
- Subcommands: `list`, `set`, `delete`.

In `crates/odm-cli/src/commands/queue.rs`:
- Subcommand `schedule` with `--start`, `--stop`, `--days`.

- [ ] **Step 3: Run CLI tests and verify gate**

Run:
```bash
cargo test -p odm-cli --test m5_cli_features
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 4: Commit**

```bash
git add crates/odm-cli
git commit -m "feat(cli): add batch expansion, checksums, mirrors and category commands"
```

---

### Task 7: GUI integration for M5 features (`odm-gui`)

**Files:**
- Modify: `crates/odm-gui/src/messages.rs`
- Modify: `crates/odm-gui/src/state.rs`
- Modify: `crates/odm-gui/src/bridge.rs`
- Modify: `crates/odm-gui/src/ui/add_dialog.rs`
- Modify: `crates/odm-gui/src/ui/settings_dialog.rs`
- Modify: `crates/odm-gui/src/ui/sidebar.rs`
- Create: `crates/odm-gui/tests/m5_gui_features.rs`

**Interfaces:**
- Produces: GUI controls for checksum input, mirrors, batch URL expansion preview, category management, and queue scheduling.

- [ ] **Step 1: Write GUI M5 state and reducer tests first**

`crates/odm-gui/tests/m5_gui_features.rs`:
- Test adding batch URL: expands into multiple jobs in state.
- Test checksum input in AddDialog prefilled and dispatched.
- Test category sync and selection filtering.

- [ ] **Step 2: Implement GUI UI components for M5**

In `crates/odm-gui/src/ui/add_dialog.rs`:
- Add inputs for Checksum (algorithm selector + hash string).
- Add inputs for Mirrors (comma/line separated).
- Show batch expansion preview when URL contains `[...]`.

In `crates/odm-gui/src/ui/settings_dialog.rs`:
- Add Category management section: customize directory per category.
- Add Queue Schedule section: configure start/stop time and days for queues.

In `crates/odm-gui/src/ui/sidebar.rs`:
- Display categories dynamically loaded from daemon with live badge counts.

- [ ] **Step 3: Run GUI tests and verify gate**

Run:
```bash
cargo test -p odm-gui --test m5_gui_features
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

- [ ] **Step 4: Commit**

```bash
git add crates/odm-gui
git commit -m "feat(gui): integrate checksums, mirrors, batch urls and category management"
```

---

### Task 8: End-to-end integration, manual checklist, changelog and milestone gate

**Files:**
- Create: `docs/manual-checklists/m5-features.md`
- Modify: `CHANGELOG.md`
- Create: `tests/m5_e2e_features.rs` (or `crates/odm-daemon/tests/e2e_m5.rs`)

- [ ] **Step 1: Create manual checklist in `docs/manual-checklists/m5-features.md`**

Document all manual verification steps:
1. Checksum verification with matching and corrupted files.
2. Mirror failover during segmented download.
3. Metalink import (`.metalink` / `.meta4`) from CLI and GUI.
4. Batch URL expansion from CLI and GUI.
5. Category automatic routing and custom extension mapping.
6. Queue scheduling with automated start/stop.

- [ ] **Step 2: Record M5 capabilities in CHANGELOG.md**

Under `## [Unreleased]` and `### Added`:
```markdown
- Cryptographic checksum verification (SHA-256, SHA-1, MD5) with streaming integrity checks in `Verifying` state.
- Multi-mirror downloads with dynamic segment distribution and automatic mirror failover.
- Metalink 3 and Meta4 XML file parser.
- Batch URL pattern expansion supporting numeric (`[001-100]`) and alphabetic ranges with Cartesian products.
- File categories mapping extensions to target directories with automatic file routing.
- Queue scheduling with configurable start/stop times and active weekdays.
- Full CLI and GUI controls for checksums, mirrors, categories, batch URLs, and schedules.
```

- [ ] **Step 3: Run full milestone gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

Expected: All pass cleanly; zero clippy warnings; zero unsafe; deny checks green.

- [ ] **Step 4: Commit**

```bash
git add docs/manual-checklists/m5-features.md CHANGELOG.md docs/superpowers/plans/2026-11-11-odm-m5-features.md
git commit -m "docs: record M5 checksums, mirrors, metalink, categories and scheduler"
```
