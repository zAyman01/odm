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
