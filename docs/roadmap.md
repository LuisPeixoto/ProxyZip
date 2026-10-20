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

## ✅ Phase 4: Ecosystem

Guided setup for iPhone and iPad (profile and QR code), Android, Java and Firefox · terminal environment for every
shell with a CA bundle and a Node.js preload · **MCP server** for AI assistants · plugin API · in-app updates ·
release workflow with optional signing, command-line archives and package-manager manifests · HTTP/3: Alt-Svc
handling, HTTP/3 to servers and an HTTP/3 reverse proxy. See [Devices](devices.md), [AI assistants](mcp.md),
[Plugins](plugins.md), [HTTP/3](http3.md) and [Releasing](distribution.md).

## ✅ Phase 5: Working with APIs

Mock servers with templates, on a host or on their own port · OpenAPI import (mocks from a specification) and contract
checks · replay suites with variables, assertions and JUnit reports, runnable from the app and in CI · Postman import ·
workspaces and secret redaction · plugin API 2 (settings, inspector tabs) and a plugin catalog · a Homebrew tap and a
Scoop bucket. See [Mocks](mocks.md), [Suites](suites.md), [Workspaces and secrets](workspaces.md) and [Plugins](plugins.md).

## Phase 6: Ideas

- [ ] Publish to winget and Flathub
- [ ] Verified, notarized installers once signing certificates are available
- [ ] Decrypt QUIC captured system-wide (a packet-capture mode), not only through the proxy
- [ ] GraphQL and gRPC mocks, and mock record-and-replay (capture a session, serve it back)
- [ ] Load and performance runs from a suite
- [ ] Shared team workspaces with sync
- [ ] Plugin signing and permissions
