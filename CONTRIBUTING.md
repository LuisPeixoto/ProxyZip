# Contributing to ProxyZip

Thank you for helping make ProxyZip better! Contributions of every size are welcome: bug reports, documentation,
design polish, tests and new features.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to contribute

- **Report a bug** with the [bug report form](https://github.com/LuisPeixoto/ProxyZip/issues/new?template=bug_report.yml).
- **Suggest a feature** with the [feature request form](https://github.com/LuisPeixoto/ProxyZip/issues/new?template=feature_request.yml).
  The [roadmap](docs/roadmap.md) shows what is already planned.
- **Improve the docs.** Typos, clearer wording and missing examples are all valuable.
- **Pick up an issue** labelled
  [`good first issue`](https://github.com/LuisPeixoto/ProxyZip/labels/good%20first%20issue) or
  [`help wanted`](https://github.com/LuisPeixoto/ProxyZip/labels/help%20wanted).

For anything bigger than a small fix, please open an issue first so we can agree on the approach.

## Development setup

You need [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 20 or newer and, on Linux, the
[Tauri system dependencies](https://tauri.app/start/prerequisites/#linux).

```bash
git clone https://github.com/LuisPeixoto/ProxyZip.git
cd ProxyZip

# Engine and CLI
cargo test --workspace --exclude proxyzip-desktop
cargo run -p proxyzip-cli -- start --port 9394

# Desktop app (hot reload for the UI)
cd apps/desktop
npm install
npm run tauri dev
```

Tips:

- Set `PROXYZIP_HOME=/tmp/pz` to use a throw-away CA and settings while developing.
- Set `PROXYZIP_LOG=debug` for verbose engine logs.
- If your system proxy already points at the port your dev build listens on, all of your machine's traffic will flow
  into that build. Use a different port for experiments.

## Project layout

| Area | Where |
|------|-------|
| Request lifecycle, TLS interception | `crates/proxyzip-proxy/src/{server,tunnel,forward,websocket}.rs` |
| Certificates | `crates/proxyzip-tls/src/ca.rs` |
| Data model and events | `crates/proxyzip-core/src` |
| Body decoding | `crates/proxyzip-codec/src/lib.rs` |
| HAR and cURL | `crates/proxyzip-export/src` |
| OS integration | `crates/proxyzip-sysproxy`, `crates/proxyzip-certinstall` |
| Desktop commands | `apps/desktop/src-tauri/src/commands.rs` |
| Interface | `apps/desktop/src/lib/components` |

Read [docs/architecture.md](docs/architecture.md) for how the pieces fit together.

## Workflow

1. Fork the repository and create a branch from `main`.
2. Make your change in small, focused commits.
3. Run the checks below.
4. Open a pull request and fill in the template.

### Branch names

Use a short prefix: `feat/…`, `fix/…`, `docs/…`, `refactor/…`, `test/…`, `ci/…`, `chore/…`.

### Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org):

```text
feat(proxy): relay WebSocket frames
fix(tls): reuse the leaf key across hosts
docs: explain certificate pinning
```

Common types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`. Keep the subject under
72 characters, in the imperative mood. Explain *why* in the body when it is not obvious.

## Checks before opening a pull request

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude proxyzip-desktop
cd apps/desktop && npm run check
```

- New proxy behaviour needs an end-to-end test in `crates/proxyzip-proxy/tests/e2e.rs`
  (a real client talking through the proxy to a real upstream).
- New logic in other crates needs unit tests next to the code.
- UI changes should include a screenshot in both the dark and the light theme.
- Update the docs and the [changelog](CHANGELOG.md) when behaviour changes.

## Code style

- Rust code is formatted with `rustfmt` (see `rustfmt.toml`) and must be clippy-clean.
- Prefer small modules and plain data types. Comments should explain *why*, not *what*.
- Never log or persist secrets, request bodies or cookies outside the in-memory store without an explicit user action.
- The UI is Svelte 5 with TypeScript and Tailwind CSS. Reuse the colour tokens defined in `src/app.css`; do not hard-code colours.

## Security

ProxyZip handles decrypted traffic and a private CA key. Please report vulnerabilities privately, as described in
[SECURITY.md](SECURITY.md), not in public issues.

## License

By contributing you agree that your contributions are licensed under the [MIT License](LICENSE).
