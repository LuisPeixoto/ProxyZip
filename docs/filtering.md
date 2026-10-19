# Filtering

The filter bar (focus it with `⌘K` / `Ctrl+K`) narrows the request list as you type. Everything is evaluated locally
and instantly.

## Syntax

Terms are separated by spaces and combined with **AND**: a request must match every term.

| Term | Meaning | Example |
|------|---------|---------|
| `text` | The URL contains `text` (case-insensitive) | `login` |
| `method:VERB` | Exact HTTP method | `method:post` |
| `status:NNN` | Exact status code | `status:404` |
| `status:Nxx` | Status class | `status:5xx` |
| `host:text` or `domain:text` | The host contains `text` | `host:api` |
| `path:text` | The path contains `text` | `path:/v2/` |
| `type:text` | The content type contains `text` | `type:json` |
| `gql:text` or `graphql:text` | The GraphQL operation (`query GetUser`) contains `text` | `gql:GetUser` |
| `/regex/` | The URL matches a regular expression (case-insensitive) | `/\/users\/\d+/` |
| `-term` | Negates any term above | `-analytics`, `-method:get` |

An unknown prefix such as `foo:bar` is treated as plain text.

## Examples

```text
host:api method:post                 POST requests to hosts containing "api"
status:4xx -host:google              client errors, except Google
type:json /\/graphql/                JSON responses from GraphQL endpoints
-host:analytics -host:tracking       hide noisy third parties
```

## Other ways to narrow the list

- **Type chips** under the toolbar (JSON, HTML, JS, CSS, Images, Fonts, Media, WebSocket, Errors, Other).
- **Sidebar**: All traffic, Pinned, Errors, WebSockets, then individual domains and client devices.

The type chips, the sidebar selection and the filter bar all apply together.
