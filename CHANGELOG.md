# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0]

Analysis.

### Added

- **Scripting**: `onRequest` / `onResponse` in JavaScript (QuickJS) with hashing, Base64, JWT and JSON helpers, a
  persistent `ctx.shared`, a 2 second and 64 MB limit per run, a Console tab and a **Test with the selected request**
  button. Errors never break traffic.
- Viewers for **Protobuf** (with `.desc` import and gRPC path matching), **gRPC and gRPC-Web**, **MessagePack**,
  **GraphQL** (operation shown in the list, searchable with `gql:`) and **server-sent events** that update live.
- **jq** filters on JSON bodies, with history.
- **Diff** of two requests and **code generation** in 14 languages.
- A CodeMirror 6 editor for bodies, breakpoints, Compose and scripts.
- **Sessions**: save and open `.pzsession` files, recent sessions, restore on launch, unsaved-changes indicator.
- **CSV** and **Postman collection** export.
- CLI: `proxyzip start --save`, `proxyzip export` and `proxyzip code`.


## [0.2.0]

Debugging tools.

### Added

- **Breakpoints** for requests and responses: pause matching traffic, edit method, URL, headers, status and body, then
  continue or abort. The editor opens automatically and the toolbar shows how many requests are waiting.
- **Compose**: build a request by hand, import a cURL command, or **Edit & repeat** a captured request.
- **Map Local** (file or folder), **Map Remote**, **Block** (status or dropped connection) and **Rewrite** rules for
  headers, query parameters, status codes and body text (decompressed, with regular expressions).
- **Throttling** with Edge, 3G, LTE and poor Wi-Fi presets or custom latency and bandwidth.
- **DNS overrides**, **upstream proxy** (HTTP with CONNECT and SOCKS5, with authentication and a bypass list),
  **reverse proxy** listeners, a **SOCKS5 listener** and a **PAC file** at `/proxy.pac`.
- **Record only** filter.
- A **Tools** panel in the desktop app, context-menu shortcuts (set breakpoint, Map Local, Map Remote, Block, Edit &
  repeat), and tags on requests that show which rules touched them.
- `proxyzip start` options `--rules`, `--socks-port`, `--upstream-proxy`, `--reverse` and `--throttle`.
- Rules are stored in `rules.json` and apply to new requests immediately.

### Changed

- Requests dropped by a block rule or aborted at a breakpoint close the client connection.
- Raw tunnels honour DNS overrides and the upstream proxy.

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

[Unreleased]: https://github.com/LuisPeixoto/ProxyZip/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/LuisPeixoto/ProxyZip/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/LuisPeixoto/ProxyZip/releases/tag/v0.1.0
