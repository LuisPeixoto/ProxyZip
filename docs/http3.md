# HTTP/3 (QUIC)

HTTP/3 runs over UDP, which a classic HTTP proxy cannot see. ProxyZip handles it in three ways. The first is on by
default and the others are opt-in.

## 1. Clients stay in the proxy (default)

Servers announce HTTP/3 with an `Alt-Svc: h3=":443"` header. A client that sees it may try QUIC directly next time and
**bypass the proxy**. ProxyZip removes the `h3` entries from responses (other alternatives stay), so your clients
keep sending traffic through it. The recorded response still shows what the server really sent.

Turn it off in **Settings → Proxy → Hide HTTP/3 offers from clients** or with `proxyzip start --keep-alt-svc`.

## 2. HTTP/3 to servers (experimental)

**Settings → Proxy → Use HTTP/3 to servers that offer it**, or `proxyzip start --http3`.

The first request to a server goes over TCP and teaches ProxyZip about the `h3` alternative (respecting `ma`, the
advertised lifetime). Later requests to that server use HTTP/3, and the flow shows `HTTP/3`. If QUIC cannot connect,
the request falls back to TCP and that server is skipped for five minutes.

Limits:

- QUIC cannot be tunnelled through an upstream HTTP or SOCKS5 proxy, so hosts that match an upstream-proxy rule
  always use TCP.
- WebSocket always uses HTTP/1.1.
- DNS overrides and the *insecure upstream* setting apply to HTTP/3 as well.

## 3. A reverse proxy that speaks HTTP/3

Tick **Also accept HTTP/3 on UDP port N** on a reverse proxy in **Tools → Proxy & ports** (or add `--reverse-h3`
to `proxyzip start --reverse PORT=URL`). ProxyZip listens on the same port number over UDP with a certificate from the
ProxyZip CA for whatever name the client asked for, and records everything like any other flow. Use it to test HTTP/3
clients such as `curl --http3` or a mobile app.

```bash
proxyzip start --reverse 8443=https://api.example.com --reverse-h3
curl --http3 --cacert "$(proxyzip ca path)" https://localhost:8443/health
```

## What is not covered

ProxyZip does not intercept QUIC that clients send on their own, because that needs a system-wide capture rather than
a proxy. With *Hide HTTP/3 offers* on, browsers and most HTTP clients fall back to HTTP/2 through the proxy.
