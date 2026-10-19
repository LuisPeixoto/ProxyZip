# Architecture

ProxyZip is a Cargo workspace. The engine is a set of small crates with one job each, and two front-ends (a CLI and
a Tauri desktop app) sit on top.

```text
                    ┌───────────────────────────────┐
                    │  apps/desktop   proxyzip-cli  │   front-ends
                    └───────────────┬───────────────┘
                                    │
  ┌────────────┬────────────┬───────┴──────┬────────────┬────────────┐
  │ proxyzip-  │ proxyzip-  │ proxyzip-    │ proxyzip-  │ proxyzip-  │
  │ export     │ sysproxy   │ certinstall  │ codec      │ rules      │
  │ HAR, cURL  │ OS proxy   │ OS trust     │ decoding   │ rule model │
  └─────┬──────┴────────────┴──────┬───────┴────────────┴─────┬──────┘
        │                          │                          │
        ▼                          ▼                          ▼
  proxyzip-core  ◀────────────  proxyzip-proxy  ─────────▶  proxyzip-tls
  Flow, store,                  tokio + hyper               CA, leaf certs,
  events                        engine                      key log
```

## Crates

| Crate | Responsibility |
|-------|----------------|
| `proxyzip-core` | The `Flow` model, a bounded in-memory `FlowStore`, and a broadcast event bus. |
| `proxyzip-proxy` | Listener, `CONNECT` handling, TLS interception, HTTP/1.1, HTTP/2, WebSocket relay, upstream client. |
| `proxyzip-tls` | Root CA creation and persistence, per-host leaf certificates with an LRU cache, TLS key logging. |
| `proxyzip-rules` | The rule model (`rules.json`), request matching, and the pure functions that decide what happens to a request and rewrite it. No I/O. |
| `proxyzip-codec` | Content-encoding removal (gzip, deflate, Brotli, Zstandard), content classification, pretty-printing. |
| `proxyzip-export` | HAR 1.2 import and export, cURL generation. |
| `proxyzip-sysproxy` | Enable, disable and read the operating-system proxy (networksetup, WinINet registry, gsettings and KDE). |
| `proxyzip-certinstall` | Trust the CA in OS and application stores, iOS Simulator setup, terminal environment. |
| `proxyzip-cli` | The `proxyzip` binary. |
| `apps/desktop/src-tauri` | Tauri 2 shell: owns the proxy, exposes commands, streams events to the UI. |
| `apps/desktop/src` | Svelte 5 + TypeScript + Tailwind CSS interface. |

## Request lifecycle

```text
client ──TCP──▶ listener (tokio)
                  │  hyper auto-detects HTTP/1.1 or HTTP/2, upgrades enabled
                  ├─ absolute-form request ───────────────────────────────▶ forward()
                  └─ CONNECT host:port → 200 → upgraded stream
                        │ peek the first bytes
                        ├─ TLS handshake and host allowed by the SSL proxying policy
                        │     read SNI and ALPN → mint a leaf certificate (cached)
                        │     → terminate TLS (h2 only if the client offered it)
                        │     → hyper ──────────────────────────────────▶ forward()
                        ├─ plain HTTP inside the tunnel ─────────────────▶ forward()
                        └─ excluded host or unknown protocol → raw byte tunnel

forward():
  resolve target (reverse proxy listeners use their fixed target) → rules.plan(request)
  → record the request (unless "record only" excludes it)
  → block ........ synthetic status, or drop the connection
  → map local .... answer from disk
  → map remote / rewrite request / breakpoint (pause until the user decides, with edits)
  → latency and upload pacing for throttling
  → upstream hyper client (rustls, bundled + native roots, ALPN h2 / http/1.1; DNS overrides and the
    upstream proxy are applied by the connector when the TCP connection is opened)
  → record the response head
  → header-only rewrite: stream the body back through a tap that keeps a bounded copy
  → body rewrite or response breakpoint: hold the whole response, decode, edit, then send
  → optional bandwidth pacing → on completion record body and timing; on client abort mark the flow as failed
  WebSocket: HTTP/1.1-only upstream client, both sides upgraded, frames relayed and recorded

extra listeners (started and stopped as the rules change):
  reverse proxy port ─▶ forward() with a fixed target
  SOCKS5 port ───────▶ handshake ─▶ the same path as CONNECT
```

## Design decisions

- **Each side negotiates independently.** The client-to-proxy protocol (HTTP/1.1 or HTTP/2) does not constrain the
  proxy-to-server protocol. hyper-util picks the wire version per upstream connection.
- **One leaf key for all hosts.** Generating a key pair per host dominates interception latency. A single key per CA
  instance, with certificates cached per host, keeps the first request to a new host fast.
- **Bodies stream through.** The proxy never buffers a response before forwarding it. The tap keeps a copy up to
  the configured limit, so event streams and large downloads behave normally.
- **Errors are flows.** TLS rejection by the client, DNS failures, upstream TLS errors and client aborts all become
  failed flows with an explanation, because that is when you need the tool most.
- **WebSocket compression is disabled** (`Sec-WebSocket-Extensions` is removed) so frames can be decoded and recorded.
- **UI event batching.** The desktop shell coalesces store events every 60 ms. If the UI falls behind the broadcast
  buffer it receives a resync signal and reloads the list.
- **Rules are data, the engine is pure.** `proxyzip-rules` turns a request description into a plan (block, map,
  rewrite operations, breakpoint, throttle profile) without touching the network, so every rule is covered by fast unit
  tests. The proxy executes the plan and is tested end to end.
- **Rules apply live.** Settings hold the rule set behind an `Arc`, replaced atomically; a request sees one consistent
  snapshot. The connector reads DNS and upstream-proxy rules at connect time, so no client has to be rebuilt.
- **Breakpoints are a rendezvous.** A paused flow waits on a one-shot channel held by the `BreakpointHub`. The UI
  resolves it with a decision. If the client disconnects while paused, the entry is dropped and the flow is marked failed.
- **Private by default.** The proxy listens on loopback unless you enable LAN connections. The CA key is stored with
  owner-only permissions.

## Testing

`crates/proxyzip-proxy/tests/e2e.rs` starts real upstream servers (HTTP, HTTPS with HTTP/2, WebSocket) and drives the
proxy with real clients. New proxy behaviour should come with a test there.
