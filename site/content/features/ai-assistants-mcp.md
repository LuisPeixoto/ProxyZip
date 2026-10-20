---
title: MCP server for HTTP traffic and AI assistants
description: A built-in Model Context Protocol server lets Claude Code, Cursor and VS Code read captured requests. Local, token-protected, redacted by default.
h1: Let your AI assistant read the traffic
image: settings
image_alt: Settings page for AI assistants with the MCP address and a masked token
---

When something fails, you often paste a request into an assistant and ask what is wrong. With ProxyZip's built-in [Model Context Protocol](https://modelcontextprotocol.io) server, the assistant can look at the captured traffic itself.

## What an assistant can do

- List and filter requests
- Read decoded bodies and run `jq` filters
- Generate code for a request
- Export a HAR file

## Safe by default

- **Off** until you enable it in *Settings → AI assistants*
- Listens on `127.0.0.1` only, and every call needs an access token
- Secrets such as tokens, cookies and passwords become `<redacted>` unless the call explicitly asks otherwise
- **Read-only**: editing rules or clearing traffic needs *Allow changes*

## Set it up

```bash
claude mcp add --transport http proxyzip http://127.0.0.1:9395/mcp \
  --header "Authorization: Bearer <token>"
```

The app shows a ready-to-copy command for Claude Code, Cursor, VS Code and Claude Desktop. From the command line, `proxyzip start --mcp 9395` captures and serves MCP together, and `proxyzip mcp --session capture.pzsession` serves a saved session read-only.

Remember that the assistant's provider receives whatever the assistant reads. Read the [MCP guide](../../docs/mcp/) for the security notes.
