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
| `--save <FILE>` | Write everything captured to a [`.pzsession`](sessions.md) file when you press `Ctrl+C`. |
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
| `--workspace <FILE>` | Add the rules of a `.pzworkspace` file. |
| `--redact` | Remove secrets from what `--har` and `--save` write. |
| `--http3` | Use [HTTP/3](http3.md) towards servers that advertise it (experimental). |
| `--keep-alt-svc` | Do not hide HTTP/3 offers from clients. |
| `--reverse-h3` | Also accept HTTP/3 on the UDP port of every `--reverse` listener. |
| `--mcp <PORT>` | Serve [MCP](mcp.md) for AI assistants on `127.0.0.1:PORT`. |
| `--mcp-allow-changes` | Let assistants edit rules and clear traffic (with `--mcp`). |
| `-v, --verbose` | Print request and response headers. |
| `-q, --quiet` | Do not print requests. |

Requests that a rule touched carry a tag in the output, such as `[blocked]`, `[map-local]` or `[rewrite, throttle]`.
Breakpoints need a person to edit the traffic, so they are only available in the desktop app.

```bash
proxyzip start --rules rules.json --throttle 3g --socks-port 1081 --reverse 8081=https://api.example.com
```

Press `Ctrl+C` to stop. If the system proxy was enabled by ProxyZip, it is turned off again.

## `proxyzip export`

Converts a HAR file or a [session](sessions.md) into another format.

```bash
proxyzip export session.pzsession -f csv -o traffic.csv
proxyzip export session.pzsession -f postman -o collection.json
proxyzip export capture.har -f session -o capture.pzsession
```

| Option | Description |
|--------|-------------|
| `-f, --format <FORMAT>` | `csv`, `postman`, `har` or `session`. |
| `-o, --output <FILE>` | Write to a file instead of standard output. Required for `session`. |
| `--redact` | Replace secrets (tokens, cookies, passwords) before writing. See [Workspaces and secrets](workspaces.md). |

## `proxyzip code`

Prints the code that sends a captured request.

```bash
proxyzip code session.pzsession --lang python --number 3
```

| Option | Description |
|--------|-------------|
| `-l, --lang <LANG>` | `curl`, `httpie`, `powershell`, `python`, `javascript`, `node-axios`, `go`, `rust`, `java`, `kotlin`, `swift`, `csharp`, `php` or `ruby`. Default `curl`. |
| `-n, --number <N>` | Which request to use; 1 is the first. |

## `proxyzip mock`

Serves an OpenAPI 3 file, or the mock servers of a workspace, as a mock API. See [Mocks](mocks.md).

```bash
proxyzip mock petstore.yaml --port 4010 --delay 200
```

| Option | Description |
|--------|-------------|
| `-p, --port <PORT>` | Port to serve on (default 4010) |
| `--host <ADDR>` | Address to listen on (default `127.0.0.1`) |
| `--delay <MS>` | Wait before every answer |

## `proxyzip validate`

Checks captured traffic against an OpenAPI file and exits with `1` if anything does not conform.

```bash
proxyzip validate api.yaml capture.pzsession
proxyzip validate api.yaml capture.har --json
```

## `proxyzip run`

Runs a [replay suite](suites.md) or a Postman collection. Exits with `1` when a request fails.

```bash
proxyzip run checkout.pzsuite --env staging --junit report.xml
```

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
| `proxyzip ca firefox` | Make Firefox trust ProxyZip in every profile. |
| `proxyzip ca android` | List Android devices (`--list`), route one through ProxyZip (`--serial S --proxy PORT`, `--clear-proxy`) and install the CA (`--system` or `--user`). |

## `proxyzip system-proxy`

```bash
proxyzip system-proxy on --port 9393
proxyzip system-proxy status
proxyzip system-proxy off
```

If ProxyZip ever exits abnormally while the system proxy is on, run `proxyzip system-proxy off`.

## `proxyzip env`

Prints shell exports that route a terminal through ProxyZip and make common runtimes trust its CA: curl, Node.js,
Python, Ruby, Go, Git, pip, Cargo, the AWS CLI, Deno and Java. See [Devices](devices.md#terminal-and-runtimes).

```bash
eval "$(proxyzip env)"
proxyzip env --shell fish | source
proxyzip env --shell powershell | Invoke-Expression
```

| Option | Description |
|--------|-------------|
| `--shell <SHELL>` | `sh` (bash, zsh), `fish`, `powershell` or `cmd`. Detected by default. |
| `--host <ADDR>`, `-p, --port <PORT>` | The proxy to point at (default `127.0.0.1:9393`). |

## `proxyzip terminal`

Opens a new terminal window whose traffic goes through ProxyZip.

## `proxyzip mcp`

Lets an AI assistant inspect traffic over MCP on stdio. See [AI assistants](mcp.md).

| Option | Description |
|--------|-------------|
| *(none)* | Forward to a running ProxyZip (the desktop app, or `proxyzip start --mcp`). |
| `--session <FILE>` | Serve a `.pzsession` or `.har` file instead, read-only. |
| `--url <URL>`, `--token <TOKEN>` | Use a specific endpoint (`PROXYZIP_MCP_TOKEN` also works). |

## `proxyzip har-to-curl <FILE>`

Prints every request in a HAR file as a cURL command.

```bash
proxyzip har-to-curl session.har > replay.sh
```
