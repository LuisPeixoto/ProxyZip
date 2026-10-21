---
title: Debug HTTPS traffic with a free, open-source proxy (ProxyZip)
published: false
description: ProxyZip is an open-source HTTPS debugging proxy for macOS, Windows and Linux. Inspect traffic, mock an API, decode gRPC and run API tests in CI.
tags: opensource, webdev, api, rust
canonical_url: https://luispeixoto.github.io/ProxyZip/features/https-debugging/
cover_image: https://luispeixoto.github.io/ProxyZip/og.png
---

Your app calls an API, something comes back wrong, and the only thing you can see is a spinner. The request is encrypted, the logs say nothing useful, and you end up guessing.

A **debugging proxy** fixes that: it sits between your app and the server, decrypts the HTTPS on your own machine and shows you every request and response. I built [ProxyZip](https://luispeixoto.github.io/ProxyZip/), a free, open-source HTTP and HTTPS debugging proxy, and this post walks through what it does using a small example.

**TL;DR**

- Capture and inspect HTTPS traffic from any app on macOS, Windows and Linux
- Pause a request, edit it and send it on
- Mock an API that does not exist yet, or validate real traffic against an OpenAPI file
- Turn captured requests into tests and run them in CI
- MIT licensed, written in Rust, no account and no telemetry

## What is ProxyZip?

ProxyZip is an HTTP and HTTPS debugging proxy with a desktop app and a command line. The engine is Rust (tokio, hyper, rustls); the app is Tauri and Svelte. The app and the `proxyzip` command run the same engine, so what you learn in one works in the other.

The example below uses a fictional shop API at `api.shopwave.dev`.

![ProxyZip showing the JSON response of GET /v1/products/sku_1042 next to the request list](https://luispeixoto.github.io/ProxyZip/images/hero-1200.webp)

## Install it

```bash
# macOS
brew install --cask luispeixoto/proxyzip/proxyzip

# Windows
scoop bucket add proxyzip https://github.com/LuisPeixoto/scoop-proxyzip
```

Installers for macOS, Windows and Linux (`.dmg`, `.msi`, `.exe`, `.AppImage`, `.deb`, `.rpm`) are on the [releases page](https://github.com/LuisPeixoto/ProxyZip/releases/latest). The command line alone installs with `cargo install --git https://github.com/LuisPeixoto/ProxyZip proxyzip-cli`.

## Inspect HTTPS traffic

HTTPS needs one extra step. ProxyZip creates a local certificate authority on your machine; once you trust it, the proxy can show you what your apps send. The private key never leaves your disk, and you can exclude any host from decryption.

```bash
proxyzip ca install
eval "$(proxyzip env)"
proxyzip start --port 19191
```

`proxyzip env` points the current terminal at the proxy and makes curl, Node.js, Python, Go, Git, Cargo and others trust the certificate. Every finished request prints as it happens:

```text
200 GET     https://api.shopwave.dev/v1/products 469B 1ms [mock]
200 GET     https://api.shopwave.dev/v1/products/sku_1042 142B 70ms [mock]
```

In the app you get the same requests as a list, with headers, cookies, timing, TLS details and the body as a JSON tree. Use `jq` filters on a body, or compare two requests side by side.

Binary protocols are covered too. gRPC, gRPC-Web and plain Protobuf messages are decoded even without the `.proto` files, which makes gRPC debugging a lot less painful.

![A gRPC-Web response decoded into numbered Protobuf fields](https://luispeixoto.github.io/ProxyZip/images/grpc-1200.webp)

## Change traffic instead of only reading it

Right-click a request to set a **breakpoint**: the next matching request stops before it leaves, and you can edit the method, URL, headers or body. **Map Local** answers with a file from your disk, **Map Remote** sends the request to another host, and rewrite rules change headers, bodies and status codes. You can also throttle to a slow network or block a host to see how your app copes.

![Context menu with Set breakpoint, Map Local, Map Remote and Block](https://luispeixoto.github.io/ProxyZip/images/menu-1200.webp)

## Mock an API that does not exist yet

When the backend is not ready, or has to fail in a specific way, a mock server answers for it: routes with status codes, headers, bodies and delays, plus a small template language. Right-click a captured response and choose **Create a mock from this response**, or import an OpenAPI 3 file.

```bash
proxyzip mock petstore.yaml --port 4010 --delay 200
proxyzip validate petstore.yaml capture.pzsession
```

`proxyzip validate` checks captured traffic against the specification and exits with `1` if anything does not conform, so it works as a CI step.

![The Mocks window with routes for products, cart and checkout](https://luispeixoto.github.io/ProxyZip/images/mocks-1200.webp)

## Turn requests into API tests

Select the requests of a flow, right-click and choose **Add to a suite**. A suite is a single `.pzsuite` file with checks on status and body, variables and environments, and values passed between steps with `jq`. It belongs in version control, and it runs headless:

```bash
proxyzip run checkout.pzsuite --env staging --junit report.xml
```

The command exits with `1` when a request fails and writes a JUnit report that CI systems can display. Postman collections run the same way.

![A suite run showing ten requests passed](https://luispeixoto.github.io/ProxyZip/images/suites-1200.webp)

## Phones, emulators and AI assistants

To debug an iPhone or Android device, point its Wi-Fi proxy at your computer and open `http://proxyzip.cert` to install the certificate. The Devices window shows the address and a QR code, and has one-click setup for simulators and emulators. See the [mobile debugging guide](https://luispeixoto.github.io/ProxyZip/features/mobile-debugging/).

ProxyZip also has a built-in [Model Context Protocol](https://modelcontextprotocol.io) server, so assistants such as Claude Code, Cursor and VS Code can read your captured traffic. It is off by default, listens on localhost only, needs an access token and redacts secrets unless you ask otherwise. The [MCP guide](https://luispeixoto.github.io/ProxyZip/features/ai-assistants-mcp/) has the details.

## Private and open

There is no account to create and nothing phones home; the only extra request is an optional, signed update check. The code is MIT licensed, so you can read it, fork it and ship it. Use it only on traffic you are allowed to inspect.

## Try it

- Website and docs: [luispeixoto.github.io/ProxyZip](https://luispeixoto.github.io/ProxyZip/)
- Source: [github.com/LuisPeixoto/ProxyZip](https://github.com/LuisPeixoto/ProxyZip)
- Downloads: [latest release](https://github.com/LuisPeixoto/ProxyZip/releases/latest)

If it saves you an afternoon of guessing, a star on GitHub helps other developers find it, and issues and pull requests are welcome. What would you want from a debugging proxy that the tools you use today do not do?
