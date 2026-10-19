# Troubleshooting

## Nothing shows up

- Check that the toolbar shows **Recording** (not **Paused**) and that the proxy address is green.
- Make sure the application actually uses a proxy. Many command-line tools ignore the system proxy; use
  `eval "$(proxyzip env)"` or pass the proxy explicitly (`curl -x http://127.0.0.1:9393 ...`).
- Some applications bypass the proxy on purpose (for example, ones that talk directly over UDP or QUIC).
  HTTP/3 interception is on the [roadmap](roadmap.md).

## A request fails with "TLS handshake with client failed"

The client refused the ProxyZip certificate. Either:

1. **The CA is not trusted by that client.** Install it (see [Certificates](certificates.md)). Browsers such as Firefox,
   and runtimes such as Node.js, Python, Java and Go, may use their own trust stores.
2. **The application uses certificate pinning.** Right-click the request and choose **Don't decrypt `<host>`**.

## "Install certificate" asks for a password or fails

- **macOS:** approve the keychain prompt. If it fails, run `proxyzip ca install` in a terminal to see the error.
- **Linux:** installing into the system store uses `pkexec`, so a polkit agent must be running. For Chrome and
  Chromium, install `libnss3-tools` (`certutil`) and make sure `~/.pki/nssdb` exists.
- **Windows:** the certificate is installed for the current user and Windows shows a confirmation dialog.

## The internet stopped working after ProxyZip crashed

The system proxy was probably left on. Turn it off:

```bash
proxyzip system-proxy off
```

On macOS you can also use *System Settings → Network → your connection → Details → Proxies*.

## "Address already in use"

Another program is listening on the port. The desktop app tries the next ten ports automatically. Otherwise pick a
different port in **Settings → Proxy** or with `proxyzip start --port <PORT>`.

## Upstream errors (502 from ProxyZip)

ProxyZip could not reach the server. The failed request lists the reason: DNS failure, connection refused, timeout or
an untrusted server certificate. For a development server with a self-signed certificate, enable
**Settings → Capture → Don't verify server certificates** (or `proxyzip start --insecure`). Only do this for servers
you control.

## Phones cannot connect

- Enable **Allow LAN connections** and restart the proxy (saving the settings does this).
- The phone and the computer must be on the same network, and your firewall must allow incoming connections to the
  proxy port.
- Use the address shown on the first-run screen, not `127.0.0.1`.

## Large downloads or streams look truncated

Bodies are always forwarded in full. The stored copy is capped at **Settings → Capture → Max captured body**
(32 MB by default), and the inspector shows a warning when a body was cut.

## Still stuck?

Run with debug logging and open an issue:

```bash
PROXYZIP_LOG=debug proxyzip start
```

Please include your operating system, how you started ProxyZip, and the relevant log lines. Remove any secrets first.
