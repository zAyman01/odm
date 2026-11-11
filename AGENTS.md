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
