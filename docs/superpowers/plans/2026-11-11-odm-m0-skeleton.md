# odm M0: Workspace Skeleton, CI and Repository Files — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **For opencode or any agent without those skills:** do the tasks in order, one at a time, every step in order. Tick each checkbox as you finish it. If a command's output does not match the "Expected" line, stop and report; do not work around it.

**Goal:** An empty but fully guarded Cargo workspace with eight crates, CI on three platforms, and the standard open source repository files, published at `https://github.com/zAyman01/odm`.

**Architecture:** A virtual Cargo workspace with all crates under `crates/`. Shared package metadata and clippy lints live in the root `Cargo.toml`; every crate inherits them. A guard test fails the build if any crate skips the shared lints or the `unsafe` ban. No product behavior is built in this milestone.

**Tech Stack:** Rust 1.99.1 (edition 2024, resolver 3), cargo-deny 0.20.2, GitHub Actions (`actions/checkout@v7`, `Swatinem/rust-cache@v2`, `EmbarkStudios/cargo-deny-action@v2`).

**Spec:** `docs/superpowers/specs/2026-11-11-odm-design.md` (sections 2, 9, 10, 11 and milestone M0 in section 12).

## Global Constraints

- Run every command in **Git Bash** from the repository root `D:\Development\Projects\IDM`. Do not use PowerShell or cmd: PowerShell's `>` writes the wrong text encoding and its `curl` is a different program.
- Crate names, exactly: `odm-core`, `odm-torrent`, `odm-media`, `odm-plugin`, `odm-api`, `odm-daemon`, `odm-cli`, `odm-gui`.
- The `odm-cli` package builds a binary named `odm`.
- Version `0.1.0`. License `MIT OR Apache-2.0`. Repository `https://github.com/zAyman01/odm`.
- Rust toolchain pinned to `1.99.1`. Edition `2024`.
- `#![forbid(unsafe_code)]` in every crate root.
- Clippy denies `unwrap_used` and `expect_used` outside tests.
- `Cargo.lock` is committed.
- No dependencies are added in this milestone. Every `Cargo.toml` has no `[dependencies]` section.
- File contents given in this plan are exact. Copy them character for character. Do not add, reword or reorder.
- One commit per task, with the commit message the task gives.
- The agent never pushes, creates a GitHub repository, or changes GitHub settings. Tasks marked **(human)** are run by the project owner.

## Out of Scope for M0

`extension/`, `webui/` (milestone M9). Release workflow (milestone M10). Any dependency, any product code.

## Review Focus

Failure modes this milestone must prevent, and where each is pinned:

1. A crate added in a later milestone forgets `[lints] workspace = true`, so `unwrap` slips in unnoticed. Pinned by test `every_crate_inherits_workspace_lints` (Task 1).
2. A crate root lacks `#![forbid(unsafe_code)]`. Pinned by test `every_crate_root_forbids_unsafe_code` (Task 1).
3. The lints are configured but do not actually fire. Pinned by the deliberate-violation check (Task 1, Step 9).
4. The developer machine and CI use different Rust versions, so code passes locally and fails in CI. Pinned by `rust-toolchain.toml` and the version check (Task 1, Step 3).
5. Windows checks files out with CRLF line endings and `cargo fmt --check` or text comparisons fail. Pinned by `.gitattributes` and the line-ending check (Task 1, Step 10).
6. CI passes on one operating system only. Pinned by the three-job check (Task 5, Step 4).

## File Structure

```
Cargo.toml                      workspace: members, shared metadata, shared lints
Cargo.lock                      generated, committed
rust-toolchain.toml             pins Rust 1.99.1 + rustfmt + clippy
clippy.toml                     lets tests use unwrap/expect
deny.toml                       cargo-deny: licenses, advisories, sources
.gitignore
.gitattributes                  LF line endings everywhere
crates/odm-core/Cargo.toml
crates/odm-core/src/lib.rs
crates/odm-core/tests/workspace_guard.rs   guard test for all crates
crates/odm-torrent/{Cargo.toml,src/lib.rs}
crates/odm-media/{Cargo.toml,src/lib.rs}
crates/odm-plugin/{Cargo.toml,src/lib.rs}
crates/odm-api/{Cargo.toml,src/lib.rs}
crates/odm-daemon/{Cargo.toml,src/main.rs}
crates/odm-cli/{Cargo.toml,src/main.rs}
crates/odm-gui/{Cargo.toml,src/main.rs}
.github/workflows/ci.yml
.github/ISSUE_TEMPLATE/bug_report.md
.github/ISSUE_TEMPLATE/feature_request.md
.github/pull_request_template.md
README.md  LICENSE-MIT  LICENSE-APACHE  CHANGELOG.md
CONTRIBUTING.md  CODE_OF_CONDUCT.md  SECURITY.md  AGENTS.md
```

---

### Task 0 (human): Install the Rust toolchain

This machine has no Rust and no C++ build tools. Both installers ask for administrator approval, so the project owner runs this task.

- [x] **Step 1: Install the Microsoft C++ build tools** (Rust on Windows needs their linker)

Run in a terminal:

```
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Expected: ends with `Successfully installed`. Takes several minutes.

- [x] **Step 2: Install rustup**

```
winget install --id Rustlang.Rustup
```

Expected: ends with `Successfully installed`.

- [x] **Step 3: Verify in a new Git Bash window**

Run: `rustup --version && cargo --version`
Expected: two version lines, the first starting with `rustup 1.`, the second with `cargo 1.`. If either says `command not found`, close every terminal, open a new one, and try again.

---

### Task 1: Workspace skeleton with guard rails

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `.gitignore`, `.gitattributes`
- Create: `crates/odm-core/Cargo.toml`, `crates/odm-core/src/lib.rs`
- Create: `crates/odm-torrent/Cargo.toml`, `crates/odm-torrent/src/lib.rs`
- Create: `crates/odm-media/Cargo.toml`, `crates/odm-media/src/lib.rs`
- Create: `crates/odm-plugin/Cargo.toml`, `crates/odm-plugin/src/lib.rs`
- Create: `crates/odm-api/Cargo.toml`, `crates/odm-api/src/lib.rs`
- Create: `crates/odm-daemon/Cargo.toml`, `crates/odm-daemon/src/main.rs`
- Create: `crates/odm-cli/Cargo.toml`, `crates/odm-cli/src/main.rs`
- Create: `crates/odm-gui/Cargo.toml`, `crates/odm-gui/src/main.rs`
- Test: `crates/odm-core/tests/workspace_guard.rs`
- Generated: `Cargo.lock`

**Interfaces:**
- Consumes: Rust toolchain from Task 0.
- Produces: eight buildable crates that later milestones fill in; the commands `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` all passing; binaries that print `<binary name> 0.1.0`.

- [x] **Step 1: Create the root configuration files**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.99"
license = "MIT OR Apache-2.0"
repository = "https://github.com/zAyman01/odm"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.1"
profile = "minimal"
components = ["rustfmt", "clippy"]
```

`clippy.toml`:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

`.gitignore`:

```
/target
```

`.gitattributes`:

```
* text=auto eol=lf
```

- [x] **Step 2: Install the pinned toolchain**

Run: `rustup toolchain install`
Expected: output mentions `1.99.1` and ends without an error.

- [x] **Step 3: Verify the pin is active**

Run: `rustc --version`
Expected: starts with `rustc 1.99.1`.

- [x] **Step 4: Create `odm-core` WITHOUT the guard rails, and write the guard test**

The guard rails are left out on purpose so the test can be seen failing first.

`crates/odm-core/Cargo.toml`:

```toml
[package]
name = "odm-core"
description = "Download engine for odm: jobs, queue, persistence and the HTTP backend."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
```

`crates/odm-core/src/lib.rs`:

```rust
//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
```

`crates/odm-core/tests/workspace_guard.rs`:

```rust
//! Guard rails: every workspace crate must inherit the shared lints and forbid unsafe code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

fn crate_dirs() -> Vec<PathBuf> {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut dirs: Vec<PathBuf> = fs::read_dir(crates_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_crate_inherits_workspace_lints() {
    for dir in crate_dirs() {
        let manifest = fs::read_to_string(dir.join("Cargo.toml"))
            .unwrap()
            .replace("\r\n", "\n");
        assert!(
            manifest.contains("[lints]\nworkspace = true"),
            "{} must contain `[lints]` followed by `workspace = true`",
            dir.display()
        );
    }
}

#[test]
fn every_crate_root_forbids_unsafe_code() {
    for dir in crate_dirs() {
        let roots: Vec<PathBuf> = ["src/lib.rs", "src/main.rs"]
            .iter()
            .map(|file| dir.join(file))
            .filter(|path| path.exists())
            .collect();
        assert!(!roots.is_empty(), "{} has no src/lib.rs or src/main.rs", dir.display());
        for root in roots {
            let source = fs::read_to_string(&root).unwrap();
            assert!(
                source.contains("#![forbid(unsafe_code)]"),
                "{} must contain `#![forbid(unsafe_code)]`",
                root.display()
            );
        }
    }
}
```

- [x] **Step 5: Run the guard test and watch it fail**

Run: `cargo test -p odm-core --test workspace_guard`
Expected: `test result: FAILED. 0 passed; 2 failed`. The messages contain `must contain \`[lints]\`` and `must contain \`#![forbid(unsafe_code)]\``.

- [x] **Step 6: Add the guard rails to `odm-core`**

Append to `crates/odm-core/Cargo.toml` (after a blank line):

```toml
[lints]
workspace = true
```

Replace `crates/odm-core/src/lib.rs` with:

```rust
//! Download engine for odm: jobs, queue, persistence and the HTTP backend.
#![forbid(unsafe_code)]
```

- [x] **Step 7: Create the other seven crates**

Every `Cargo.toml` below uses this template, with `NAME` and `DESCRIPTION` replaced from the table:

```toml
[package]
name = "NAME"
description = "DESCRIPTION"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[lints]
workspace = true
```

| NAME | Kind | DESCRIPTION |
|---|---|---|
| `odm-torrent` | lib | `Torrent backend for odm, built on librqbit.` |
| `odm-media` | lib | `HLS, DASH and yt-dlp backends for odm.` |
| `odm-plugin` | lib | `External-process plugin host for odm.` |
| `odm-api` | lib | `Request, response and event types shared by the odm daemon and its clients.` |
| `odm-daemon` | bin | `The odm background service: REST and WebSocket API over the download engine.` |
| `odm-cli` | bin | `Command-line client for the odm daemon.` |
| `odm-gui` | bin | `Desktop client for the odm daemon.` |

For `odm-cli` only, insert this block between the `[package]` block and the `[lints]` block, separated by blank lines:

```toml
[[bin]]
name = "odm"
path = "src/main.rs"
```

Each **lib** crate gets `src/lib.rs` with exactly two lines, where `DESCRIPTION` is the same text as in the table:

```rust
//! DESCRIPTION
#![forbid(unsafe_code)]
```

Each **bin** crate gets `src/main.rs`, where `DESCRIPTION` is the same text as in the table:

```rust
//! DESCRIPTION
#![forbid(unsafe_code)]

fn main() {
    println!("{} {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
}
```

- [x] **Step 8: Run all checks and watch them pass**

Run: `cargo fmt --all --check`
Expected: no output, exit code 0. If it prints a diff instead, run `cargo fmt --all` once and run the check again; formatting is the one thing you may fix this way.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: ends with a `Finished` line, no `warning` or `error` lines.

Run: `cargo test --workspace`
Expected: contains `test result: ok. 2 passed; 0 failed` for `workspace_guard`, and no `FAILED`.

Run: `cargo run -q -p odm-cli && cargo run -q -p odm-daemon && cargo run -q -p odm-gui`
Expected, exactly:

```
odm 0.1.0
odm-daemon 0.1.0
odm-gui 0.1.0
```

Run: `cargo metadata --no-deps --format-version 1 | grep -o '"name":"odm-[a-z]*"' | sort -u | wc -l`
Expected: `8`

- [x] **Step 9: Commit, then prove the lints really fire**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml clippy.toml .gitignore .gitattributes crates
git add --renormalize .
git commit -m "chore: add workspace skeleton with guard rails"
```

Now make two deliberate violations, one at a time.

First, append these lines to `crates/odm-core/src/lib.rs`:

```rust

pub fn violation(value: Option<u8>) -> u8 {
    value.unwrap()
}
```

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: fails. The output contains `clippy::unwrap_used`.

Run: `git restore crates/odm-core/src/lib.rs`

Second, append these lines to `crates/odm-core/src/lib.rs`:

```rust

pub fn violation() {
    unsafe {}
}
```

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: fails. The output contains `unsafe_code`.

Undo it and confirm nothing is left behind:

Run: `git restore crates/odm-core/src/lib.rs && git status --short`
Expected: no output.

- [x] **Step 10: Verify line endings**

Run: `git ls-files --eol | grep -v "i/lf" | wc -l`
Expected: `0`

---

### Task 2: License and advisory checks, CI workflow

**Files:**
- Create: `deny.toml`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the workspace from Task 1.
- Produces: `cargo deny check` passing locally; a CI workflow with jobs named `check` (matrix of three operating systems) and `deny`. Task 5 verifies it on GitHub.

- [x] **Step 1: Install cargo-deny**

Run: `cargo install cargo-deny --version 0.20.2 --locked`
Expected: ends with `Installed package \`cargo-deny v0.20.2\``. Takes several minutes.

- [x] **Step 2: Run it before the config exists and watch it fail**

Run: `cargo deny check`
Expected: fails; the output reports license errors for the `odm-*` crates because no license is allowed yet (or reports that no config was found, followed by the same license errors).

- [x] **Step 3: Create `deny.toml`**

```toml
[graph]
all-features = true

[advisories]
ignore = []

[licenses]
allow = ["MIT", "Apache-2.0"]
confidence-threshold = 0.8

[bans]
multiple-versions = "warn"
wildcards = "allow"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

- [x] **Step 4: Run it again and watch it pass**

Run: `cargo deny check`
Expected: last line is `advisories ok, bans ok, licenses ok, sources ok`. Warnings about an allowed license that was not encountered are acceptable.

- [x] **Step 5: Create `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: check (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@v7
      - name: Install pinned toolchain
        run: rustup toolchain install
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets --locked -- -D warnings
      - name: Test
        run: cargo test --workspace --locked

  deny:
    name: deny
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: EmbarkStudios/cargo-deny-action@v2
```

- [x] **Step 6: Verify the file is valid YAML with the expected jobs**

Run: `grep -c -e "^  check:" -e "^  deny:" -e "ubuntu-latest, windows-latest, macos-latest" -e "--locked" .github/workflows/ci.yml; grep -c "$(printf '\t')" .github/workflows/ci.yml`
Expected: `5` then `0` (five matching lines, no tab characters).

- [x] **Step 7: Commit**

```bash
git add deny.toml .github/workflows/ci.yml
git commit -m "ci: add cargo-deny config and CI workflow"
```

---

### Task 3: Licenses, README, changelog

**Files:**
- Create: `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the license files that `license = "MIT OR Apache-2.0"` refers to; a README that later milestones extend.

- [x] **Step 1: Download the Apache license and verify it byte for byte**

Do not type this file by hand.

Run: `curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o LICENSE-APACHE && sha256sum LICENSE-APACHE`
Expected: `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30 *LICENSE-APACHE` (a space instead of `*` is also correct). If the hash differs, delete the file, stop and report.

- [x] **Step 2: Create `LICENSE-MIT`**

```
MIT License

Copyright (c) 2026 Mahmoud Ayman

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [x] **Step 3: Create `README.md`**

````markdown
# odm — Open Download Manager

A fast, open source download manager written in Rust. It aims to replace Internet Download Manager and add torrents, streaming video, plugins and remote control.

> **Status: early development.** Nothing is usable yet. The design is in [docs/superpowers/specs/2026-11-11-odm-design.md](docs/superpowers/specs/2026-11-11-odm-design.md).

## Planned for v0.1

- Segmented HTTP and HTTPS downloads with pause, resume and automatic retry
- Queues, scheduler and speed limits
- Torrents (magnet links and `.torrent` files)
- HLS and DASH streams, and sites supported by yt-dlp
- Checksum verification, mirrors and Metalink
- Browser extension for Chrome and Firefox
- Desktop app, command-line client and a web page for remote control
- Plugins for resolving links on sites odm does not know

odm does not download DRM-protected streams.

## Layout

| Path | Contents |
|---|---|
| `crates/odm-core` | Download engine |
| `crates/odm-torrent` | Torrent backend |
| `crates/odm-media` | HLS, DASH and yt-dlp backends |
| `crates/odm-plugin` | Plugin host |
| `crates/odm-api` | Types shared by the daemon and clients |
| `crates/odm-daemon` | Background service with the HTTP API |
| `crates/odm-cli` | Command-line client (`odm`) |
| `crates/odm-gui` | Desktop client |

## Building

Install [rustup](https://rustup.rs). The pinned Rust version installs itself on first build.

```
cargo build --workspace
cargo test --workspace
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
````

- [x] **Step 4: Create `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Workspace skeleton, CI and repository files.
```

- [x] **Step 5: Verify**

Run: `head -1 LICENSE-MIT && grep -c "Mahmoud Ayman" LICENSE-MIT && grep -c "LICENSE-APACHE" README.md && grep -c "Unreleased" CHANGELOG.md`
Expected:

```
MIT License
1
1
1
```

- [x] **Step 6: Commit**

```bash
git add LICENSE-MIT LICENSE-APACHE README.md CHANGELOG.md
git commit -m "docs: add licenses, README and changelog"
```

---

### Task 4: Community and agent files

**Files:**
- Create: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `AGENTS.md`
- Create: `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`, `.github/pull_request_template.md`

**Interfaces:**
- Consumes: the check commands from Task 1 and `cargo deny check` from Task 2.
- Produces: `AGENTS.md`, the rule file every later milestone's implementing agent follows.

- [x] **Step 1: Create `AGENTS.md`**

````markdown
# Instructions for coding agents

odm is a download manager written in Rust. The design is in `docs/superpowers/specs/2026-11-11-odm-design.md`. Implementation plans are in `docs/superpowers/plans/`.

## Rules

1. Work on one plan task at a time, in order, every step in order. Do exactly what the task says. Do not add features, files, dependencies or refactors the task does not name.
2. Write the task's test first and run it to see it fail before writing the code.
3. If a command's output does not match the plan's "Expected" line, stop and report. Do not weaken or delete a test, and do not work around the failure.
4. Before finishing a task, all of these must pass:

   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

5. Never write `unsafe`. Never call `unwrap()` or `expect()` outside tests; return a `Result` instead. Never add `#[allow(...)]` outside test files.
6. Every file in a crate's `tests/` folder starts with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
7. Every new crate has `[lints]` with `workspace = true` in its `Cargo.toml` and `#![forbid(unsafe_code)]` in its root source file.
8. Add only the dependencies the task names, at the exact version it gives. After adding one, run `cargo deny check`.
9. Never change the `Backend` trait in `odm-core` or any type in `odm-api` unless the task says so.
10. Tests never use the real internet.
11. One commit per task, with the commit message the task gives. Commit `Cargo.lock` whenever it changes.
12. Never push, create a repository, create a tag, publish a crate or change GitHub settings.
````

- [x] **Step 2: Create `CONTRIBUTING.md`**

````markdown
# Contributing to odm

Thank you for helping. This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- For a bug, open an issue with steps to reproduce it.
- For a feature or a large change, open an issue first so the approach can be agreed before you write code.
- The design is in [docs/superpowers/specs/2026-11-11-odm-design.md](docs/superpowers/specs/2026-11-11-odm-design.md). Changes should fit it, or the issue should explain why the design should change.

## Setup

Install [rustup](https://rustup.rs). On Windows, also install the Visual Studio C++ build tools. The pinned Rust version installs itself on first build.

## Checks

Every pull request must pass these, which CI also runs on Windows, Linux and macOS:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Code rules

- No `unsafe` code.
- No `unwrap()` or `expect()` outside tests. Return a `Result`.
- New behavior comes with a test. Tests never use the real internet.
- A new dependency must have a license allowed in `deny.toml`. Explain in the pull request why it is needed.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org) messages, for example `feat: add speed limiter` or `fix: resume after ETag change`.
- Keep a pull request to one change. Add a line to `CHANGELOG.md` under `Unreleased` for anything a user would notice.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed under the MIT license and the Apache License, Version 2.0, without any additional terms or conditions.
````

- [x] **Step 3: Download the Code of Conduct and set the contact**

Do not type this file by hand.

Run: `gh api codes_of_conduct/contributor_covenant --jq .body > CODE_OF_CONDUCT.md`

Run: `sed -i 's|\[INSERT CONTACT METHOD\]|https://github.com/zAyman01/odm/security/advisories/new|' CODE_OF_CONDUCT.md`

Run: `head -1 CODE_OF_CONDUCT.md && grep -c "INSERT" CODE_OF_CONDUCT.md; grep -c "zAyman01/odm/security/advisories/new" CODE_OF_CONDUCT.md`
Expected:

```
# Contributor Covenant Code of Conduct
0
1
```

- [x] **Step 4: Create `SECURITY.md`**

```markdown
# Security Policy

## Supported versions

odm is in early development. Only the latest release and the `main` branch receive security fixes.

## Reporting a vulnerability

Do not open a public issue for a security problem.

Report it privately at <https://github.com/zAyman01/odm/security/advisories/new>.

Include what you found, how to reproduce it, and the version or commit you tested. You can expect a first reply within 7 days.

## Scope notes

- odm plugins are ordinary programs that run with the user's full permissions. This is by design and documented; a plugin doing harm is not a vulnerability in odm.
- The daemon's API is protected by a token and listens only on the local machine by default. Ways to reach the API without the token, or from a website open in the user's browser, are in scope.
```

- [x] **Step 5: Create the issue and pull request templates**

`.github/ISSUE_TEMPLATE/bug_report.md`:

```markdown
---
name: Bug report
about: Something does not work as it should
labels: bug
---

**What happened**

**What you expected**

**Steps to reproduce**

1.
2.

**Version and system**

- odm version or commit:
- Operating system:
- Client used (GUI, CLI, web UI, extension):

**Logs or error text**
```

`.github/ISSUE_TEMPLATE/feature_request.md`:

```markdown
---
name: Feature request
about: Suggest something odm should do
labels: enhancement
---

**The problem you want solved**

**What you would like odm to do**

**Alternatives you considered**
```

`.github/pull_request_template.md`:

```markdown
## What this changes

## Why

Closes #

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] New behavior has a test
- [ ] `CHANGELOG.md` updated if a user would notice the change
```

- [x] **Step 6: Verify every repository file from spec section 11 exists**

Run: `for f in README.md LICENSE-MIT LICENSE-APACHE CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md CHANGELOG.md AGENTS.md deny.toml .github/ISSUE_TEMPLATE/bug_report.md .github/ISSUE_TEMPLATE/feature_request.md .github/pull_request_template.md; do test -s "$f" || echo "MISSING $f"; done; echo done`
Expected: `done` and no `MISSING` line.

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny check`
Expected: no `FAILED`, no `error`, last line `advisories ok, bans ok, licenses ok, sources ok`.

- [x] **Step 7: Commit**

```bash
git add AGENTS.md CONTRIBUTING.md CODE_OF_CONDUCT.md SECURITY.md .github/ISSUE_TEMPLATE .github/pull_request_template.md
git commit -m "docs: add contributor, security and agent guidelines"
```

---

### Task 5 (human): Publish and confirm CI

Creating a public repository is visible to everyone and is the project owner's action. The agent stops after Task 4.

- [x] **Step 1: Confirm the working tree is clean**

Run: `git status --short`
Expected: no output.

- [x] **Step 2: Create the public repository and push**

Run: `gh repo create zAyman01/odm --public --source . --remote origin --push --description "Open Download Manager: a fast, open source download manager written in Rust"`
Expected: prints `https://github.com/zAyman01/odm` and a line showing `main` was pushed.

- [x] **Step 3: Enable private vulnerability reporting** (the contact named in `SECURITY.md` and `CODE_OF_CONDUCT.md`)

Run: `gh api -X PUT repos/zAyman01/odm/private-vulnerability-reporting && echo enabled`
Expected: `enabled`

- [x] **Step 4: Wait for CI and check all four jobs**

Run: `gh run watch --exit-status $(gh run list --workflow CI --limit 1 --json databaseId --jq '.[0].databaseId')`
Expected: ends with success, and four jobs each marked with a check: `check (ubuntu-latest)`, `check (windows-latest)`, `check (macos-latest)`, `deny`.

If any job fails, do not start milestone M1. Copy the failing job's log (`gh run view --log-failed`) and bring it to the planning model.

---

## Milestone Gate

M0 is complete when Task 5 Step 4 shows four green jobs. Then the planning model reviews the diff and writes the M1 plan.
