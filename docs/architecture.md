# Architecture

ProxyZip is a Cargo workspace. The engine is a set of small crates with one job each, and two front-ends (a CLI and
a Tauri desktop app) sit on top.

```text
                ┌───────────────────────────────┐
                │  apps/desktop   proxyzip-cli  │   front-ends
                └──────────────┬────────────────┘
                               │
  ┌──────────────┬─────────────┼──────────────┬──────────────────┐
  │ proxyzip-    │ proxyzip-   │ proxyzip-    │ proxyzip-        │
  │ export       │ sysproxy    │ certinstall  │ codec            │
  │ HAR, cURL    │ OS proxy    │ OS trust     │ body decoding    │
  └──────┬───────┴─────────────┴──────┬───────┴──────────────────┘
         │                            │
         ▼                            ▼
   proxyzip-core  ◀──────────  proxyzip-proxy  ──────────▶  proxyzip-tls
   Flow, store,                tokio + hyper                 CA, leaf certs,
   events                      engine                        key log
```

## Crates

| Crate | Responsibility |
|-------|----------------|
| `proxyzip-core` | The `Flow` model, a bounded in-memory `FlowStore`, and a broadcast event bus. |
| `proxyzip-proxy` | Listener, `CONNECT` handling, TLS interception, HTTP/1.1, HTTP/2, WebSocket relay, upstream client. |
| `proxyzip-tls` | Root CA creation and persistence, per-host leaf certificates with an LRU cache, TLS key logging. |
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
  resolve target → record the request → remove hop-by-hop headers
  → upstream hyper client (rustls, bundled + native roots, ALPN h2 / http/1.1)
  → record the response head → stream the body back through a tap that keeps a bounded copy
  → on completion record body and timing; on client abort mark the flow as failed
  WebSocket: HTTP/1.1-only upstream client, both sides upgraded, frames relayed and recorded
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
- **Private by default.** The proxy listens on loopback unless you enable LAN connections. The CA key is stored with
  owner-only permissions.

## Testing

`crates/proxyzip-proxy/tests/e2e.rs` starts real upstream servers (HTTP, HTTPS with HTTP/2, WebSocket) and drives the
proxy with real clients. New proxy behaviour should come with a test there.
