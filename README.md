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
