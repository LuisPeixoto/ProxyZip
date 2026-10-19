# Command line

The `proxyzip` command runs the same engine as the desktop app, without a window. It is useful on servers, in CI and
for quick captures.

```bash
cargo install --git https://github.com/LuisPeixoto/ProxyZip proxyzip-cli
```

Global option: `--home <DIR>` (or `PROXYZIP_HOME`) selects the data directory that holds the CA.
Set `PROXYZIP_LOG=debug` for engine logs on stderr.

## `proxyzip start`

Runs the proxy in the foreground and prints each finished request.

```bash
proxyzip start
proxyzip start --port 8899 --system-proxy --har session.har
proxyzip start --exclude '*.apple.com' --filter '*api*' --verbose
```

| Option | Description |
|--------|-------------|
| `--host <ADDR>` | Address to listen on (default `127.0.0.1`). Use `0.0.0.0` to accept devices on your network. |
| `-p, --port <PORT>` | Port (default `9393`). |
| `--system-proxy` | Point the operating-system proxy at ProxyZip while running. Restored on exit. |
| `--har <FILE>` | Write everything captured to a HAR file when you press `Ctrl+C`. |
| `--exclude <HOST>` | Never decrypt this host. Wildcards allowed. Repeatable. |
| `--include <HOST>` | Decrypt only these hosts and tunnel the rest. Wildcards allowed. Repeatable. |
| `--filter <GLOB>` | Only print requests whose URL matches the wildcard. |
| `--insecure` | Do not verify upstream server certificates. |
| `--keylog <FILE>` | Append TLS secrets in NSS key log format, for decrypting captures in Wireshark. |
| `--rules <FILE>` | Load [rules](tools.md#rules-file) (block, map, rewrite, throttling, …) from a JSON file. The options below add to them. |
| `--socks-port <PORT>` | Also accept SOCKS5 clients on this port. |
| `--upstream-proxy <URL>` | Chain through another proxy: `http://[user:pass@]host:port` or `socks5://[user:pass@]host:port`. |
| `--reverse <PORT=URL>` | Expose a server on a local port, e.g. `8081=https://api.example.com`. Repeatable. |
| `--throttle <PRESET>` | Simulate a slow network: `edge`, `3g`, `4g` or `wifi-poor`. |
| `-v, --verbose` | Print request and response headers. |
| `-q, --quiet` | Do not print requests. |

Requests that a rule touched carry a tag in the output, such as `[blocked]`, `[map-local]` or `[rewrite, throttle]`.
Breakpoints need a person to edit the traffic, so they are only available in the desktop app.

```bash
proxyzip start --rules rules.json --throttle 3g --socks-port 1081 --reverse 8081=https://api.example.com
```

Press `Ctrl+C` to stop. If the system proxy was enabled by ProxyZip, it is turned off again.

## `proxyzip ca`

| Command | Description |
|---------|-------------|
| `proxyzip ca path` | Print the certificate file path. |
| `proxyzip ca show` | Print the certificate in PEM format. |
| `proxyzip ca install` | Trust the certificate in the operating-system store. |
| `proxyzip ca uninstall` | Remove it from the operating-system store. |
| `proxyzip ca status` | Show whether the certificate is trusted. |
| `proxyzip ca regenerate` | Replace the CA with a new one. |
| `proxyzip ca ios-simulator` | Install into the booted iOS Simulator (macOS with Xcode). |

## `proxyzip system-proxy`

```bash
proxyzip system-proxy on --port 9393
proxyzip system-proxy status
proxyzip system-proxy off
```

If ProxyZip ever exits abnormally while the system proxy is on, run `proxyzip system-proxy off`.

## `proxyzip env`

Prints shell exports that route a terminal through ProxyZip and make common runtimes trust its CA.

```bash
eval "$(proxyzip env)"
```

## `proxyzip har-to-curl <FILE>`

Prints every request in a HAR file as a cURL command.

```bash
proxyzip har-to-curl session.har > replay.sh
```
