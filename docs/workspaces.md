# Workspaces and secrets

## Share your setup: workspaces

A **workspace** (`.pzworkspace`) is your configuration in one file, so a team can use the same setup:

- all rules: block, Map Local and Remote, rewrite, breakpoints, scripts, mocks, DNS, reverse proxies;
- imported Protobuf descriptors and message mappings;
- the SSL proxying include and exclude lists;
- which plugins are switched on (the plugins themselves are installed from the [catalog](plugins.md#catalog)).

It does not contain captured traffic (use a session for that) or your settings such as the port.

In the app: **Export ▾ → Export rules and settings as a workspace…** and **Load a workspace (replace)** or **(add to
current)**. Adding appends the rules to the ones you have. On the command line, `proxyzip start --workspace team.pzworkspace`
adds its rules to the proxy.

Credentials that rules carry (an `Authorization` header set by a rewrite, the upstream proxy password) are replaced by
`<redacted>` when you export, so a workspace is safe to commit.

## Share a capture: remove secrets

Captured traffic is full of secrets: cookies, bearer tokens, passwords in forms, API keys in query strings. ProxyZip
can remove them and keep the shape of the traffic.

| Where | What happens |
|-------|--------------|
| **Export → HAR, CSV, Postman** | Secrets are removed when **Settings → Remove secrets when exporting** is on (the default) |
| **Export → Save … as a session without secrets…** | A `.pzsession` copy with secrets removed. Your own saves keep everything |
| `proxyzip export … --redact`, `proxyzip start --redact` | The same on the command line |
| [AI assistants](mcp.md) | Always, unless the assistant asks for raw data (`redact: false`) |

What is replaced with `<redacted>`:

- **Headers:** `Authorization` and `Proxy-Authorization` (the scheme stays: `Bearer <redacted>`), `Cookie` and
  `Set-Cookie` (names stay, values go), `X-API-Key`, and any header whose name contains `token`, `secret`, `key`,
  `session`, `auth`, `password`, `credential` or `signature`.
- **Query strings and form bodies:** `token`, `access_token`, `password`, `secret`, `api_key`, `key`, `sig`, `code`,
  `session` and similar.
- **JSON bodies:** fields with those names, at any depth.
- **Anywhere in text bodies:** JSON Web Tokens.

Compressed bodies are decoded first, so a gzip response is cleaned too; the stored copy is then plain. Binary bodies
are left as they are.

> Redaction works by name and pattern. It is a strong default, not a guarantee: a secret in an unusual field, or inside
> an image, is not found. Look at what you share.
