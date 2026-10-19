# Sessions and export

A **session** is everything you captured, saved to one file so you can close the app, share the capture or come back
later.

## Save and open

| Action | Shortcut |
|--------|----------|
| Save the session | `⌘S` / `Ctrl+S` |
| Open a session | `⌘O` / `Ctrl+O` |

Sessions are `.pzsession` files: JSON compressed with Zstandard, with bodies kept byte for byte. Opening one lets
you **replace** the current list or **add** to it. A dot on the Save button means there are unsaved changes.

Recent sessions appear on the welcome screen. In **Settings** you can turn on *Restore the last session on launch*:
ProxyZip then saves the session when you quit and reloads it next time.

> Sessions can contain tokens and personal data. Treat them like the traffic itself before you share them.

## Export

**Export** in the toolbar writes the selected requests, or all of them:

| Format | Use it for |
|--------|------------|
| HAR 1.2 | Browsers' dev tools and most other tools |
| CSV | Spreadsheets: the list columns, with cells that start with `=`, `+`, `-` or `@` protected against formula injection |
| Postman collection v2.1 | One request per item, grouped by host |

**Import** reads HAR files.

## From the command line

```bash
proxyzip start --save session.pzsession          # save on Ctrl+C
proxyzip export session.pzsession -f csv -o traffic.csv
proxyzip export session.pzsession -f postman -o collection.json
proxyzip export capture.har -f session -o capture.pzsession
```

See [Command line](cli.md) for every option.
