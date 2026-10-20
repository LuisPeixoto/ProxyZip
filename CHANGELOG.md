# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0]

Working with APIs.

### Added

- **Mock servers**: answer requests from your own routes (`/users/:id`, `*`, `/files/**`) with status, headers, body,
  delay and **templates** (`{{params.id}}`, `{{query.x}}`, `{{body.name}}`, `{{uuid}}`, `{{now}}`, `{{random_int 1 100}}`).
  A mock can capture a host or have a port of its own. Right-click a response → **Create a mock from this response**.
- **OpenAPI**: import a 3.0 or 3.1 file (YAML or JSON) as a mock server, with examples generated from the schemas, and
  **check captured traffic against a specification**. `proxyzip mock` and `proxyzip validate` (which exits with 1 on a
  violation) work in CI.
- **Replay suites** (`.pzsuite`): ordered requests with variables, environments, checks (status, header, jq, body, duration,
  OpenAPI) and values passed between requests. Build them from captured requests or a Postman collection, run them in the
  app or with `proxyzip run --junit report.xml`.
- **Workspaces** (`.pzworkspace`): rules, Protobuf descriptors, SSL lists and enabled plugins in one file, with credentials
  hidden. `proxyzip start --workspace`.
- **Secret redaction** for HAR, CSV, Postman and sessions (`--redact`, **Save without secrets**), and for everything sent
  to AI assistants by default.
- **Plugin API 2**: settings with a form in the app, tabs in the inspector, and a **plugin catalog** whose downloads are
  verified with SHA-256. Three new plugins: Base64 viewer, Add header, Request timeline.
- A Homebrew tap and a Scoop bucket, and a workflow that updates them when a release is published.

### Changed

- The example plugins moved to `plugins/` and became the catalog.
- The desktop app now trusts the system's web roots for its own downloads (catalog).


## [0.4.0]

Ecosystem.

### Added

- **Devices & apps**: guided setup for iPhone and iPad (configuration profile and QR code), the iOS Simulator, Android
  emulators and devices (`adb` proxy routing, user and system certificates), Java (a trust store built from your JDK) and
  Firefox (every profile), with a status check for each step.
- **Terminal and runtimes**: `proxyzip env` now supports bash, zsh, fish, PowerShell and cmd, uses a CA bundle so hosts
  you excluded keep working, covers Git, pip, Cargo, the AWS CLI, Deno and Java, and preloads a small agent that makes
  Node.js older than 24 honour the proxy. `proxyzip terminal` and **Open a terminal** start a ready-made session.
- **MCP server** for AI assistants (Claude Code, Cursor, VS Code, Claude Desktop): list, search and read requests, run
  jq filters, generate code and, when allowed, edit rules. Loopback only, token protected, off by default.
  `proxyzip mcp`, `proxyzip start --mcp`.
- **Plugins**: traffic hooks, body viewers and “copy as” formats written in JavaScript, installed from a folder or a
  `.zip`, with two examples.
- **In-app updates**, signed with a separate update key, with a check at startup that can be turned off.
- **HTTP/3**: `Alt-Svc` offers are hidden from clients so they keep using the proxy, servers that offer HTTP/3 can be
  reached over QUIC (experimental), and reverse proxies can accept HTTP/3.
- Release workflow: optional macOS and Windows signing, command-line archives for five targets, and Homebrew, Scoop,
  winget and Flatpak manifests generated from the release.

### Changed

- The proxy-to-client body type is now boxed, so HTTP/1.1, HTTP/2 and HTTP/3 share one request path.
- The default port of the MCP server is 9395 (the SOCKS5 listener keeps 9394).

### Fixed

- End-to-end tests no longer depend on the operating system's proxy settings.

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
