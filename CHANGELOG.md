# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0]

First public release.

### Added

- Intercepting proxy engine in Rust: HTTP/1.1, HTTP/2 and WebSocket, with HTTPS decrypted through a local certificate
  authority.
- Per-host certificate generation with caching, SSL proxying include and exclude lists with wildcards, and
  `SSLKEYLOGFILE` output.
- Content decoding for gzip, deflate, Brotli and Zstandard.
- HAR 1.2 import and export, and cURL generation.
- Operating-system proxy control for macOS, Windows and Linux.
- Certificate installation for macOS, Windows, Linux and the iOS Simulator, and terminal environment setup.
- Desktop app (Tauri 2 and Svelte 5): virtualized request list, filter language, domain and client sidebar,
  inspector with JSON tree, hex, image and HTML preview, timing and TLS details, WebSocket timeline, notes, pins,
  settings, and dark and light themes.
- `proxyzip` command-line tool.
- Continuous integration for Linux, macOS and Windows.

[Unreleased]: https://github.com/LuisPeixoto/ProxyZip/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/LuisPeixoto/ProxyZip/releases/tag/v0.1.0
