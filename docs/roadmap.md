# Roadmap

Items are roughly in priority order within each phase. Ideas and pull requests are welcome. Open an issue to discuss
anything large first.

## ✅ Phase 1: Capture and inspect

HTTP/1.1, HTTP/2 and WebSocket capture · HTTPS interception with a local CA · SSL proxying include and exclude lists ·
system proxy and certificate install on macOS, Windows and Linux · gzip, deflate, Brotli and Zstandard decoding ·
desktop app with list, inspector, filters, JSON tree, hex view and image/HTML preview · copy as cURL · repeat ·
HAR import and export · notes and pins · command-line tool.

## Phase 2: Debugging tools

- [ ] **Breakpoints:** pause a request or response, edit it, then continue or abort
- [ ] **Compose:** build or edit and send requests, with cURL import
- [ ] **Map Local** (file or directory) and **Map Remote** (rewrite host, path and port)
- [ ] **Block and allow lists**, **DNS overrides**, **network throttling** presets
- [ ] **Upstream proxy** (HTTP and SOCKS, with authentication) and **reverse proxy** mode
- [ ] **SOCKS5** listener and PAC file serving
- [ ] Rewrite rules for headers, status and bodies

## Phase 3: Analysis

- [ ] Scripting in JavaScript (`onRequest` / `onResponse`) with a library of helpers
- [ ] Previewers for Protobuf (with descriptor import), gRPC, MessagePack, GraphQL and server-sent events
- [ ] Diff two requests · code generation for popular languages
- [ ] JSONPath / jq filters on bodies · a full code editor for large bodies
- [ ] Persistent sessions and workspaces · import from other proxy session formats and CSV

## Phase 4: Ecosystem

- [ ] Guided setup for physical iOS devices, the Android emulator, Java and Firefox
- [ ] Terminal injection for Node.js, Python and Ruby without environment changes
- [ ] **MCP server** so AI assistants can query captured traffic
- [ ] Plugin API, auto-update, signed installers and package-manager distribution (Homebrew, Scoop, winget, Flathub)
- [ ] HTTP/3 (QUIC) interception
