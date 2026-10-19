# Roadmap

Items are roughly in priority order within each phase. Ideas and pull requests are welcome. Open an issue to discuss
anything large first.

## ✅ Phase 1: Capture and inspect

HTTP/1.1, HTTP/2 and WebSocket capture · HTTPS interception with a local CA · SSL proxying include and exclude lists ·
system proxy and certificate install on macOS, Windows and Linux · gzip, deflate, Brotli and Zstandard decoding ·
desktop app with list, inspector, filters, JSON tree, hex view and image/HTML preview · copy as cURL · repeat ·
HAR import and export · notes and pins · command-line tool.

## ✅ Phase 2: Debugging tools

Breakpoints for requests and responses · Compose with cURL import and Edit & repeat · Map Local (file or folder) ·
Map Remote · Block lists · Rewrite rules (headers, query, status, body) · Throttling presets · DNS overrides ·
Upstream proxy (HTTP and SOCKS5) · Reverse proxy · SOCKS5 listener · PAC file · Record-only filter. See
[Tools](tools.md).

## ✅ Phase 3: Analysis

JavaScript scripting with helpers · Protobuf (with descriptor import), gRPC, gRPC-Web, MessagePack, GraphQL and live
server-sent events viewers · jq filters · Diff · code generation in 14 languages · CodeMirror editor for bodies ·
`.pzsession` sessions with recent and restore-on-launch · CSV and Postman export · `proxyzip export` and
`proxyzip code`. See [Scripting](scripting.md), [Inspecting bodies](inspecting.md) and [Sessions](sessions.md).

## Phase 4: Ecosystem

- [ ] Guided setup for physical iOS devices, the Android emulator, Java and Firefox
- [ ] Terminal injection for Node.js, Python and Ruby without environment changes
- [ ] **MCP server** so AI assistants can query captured traffic
- [ ] Plugin API, auto-update, signed installers and package-manager distribution (Homebrew, Scoop, winget, Flathub)
- [ ] HTTP/3 (QUIC) interception
