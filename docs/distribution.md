# Releasing and distributing

This page is for maintainers. Pushing a tag such as `v0.4.0` runs the **Release** workflow, which builds and attaches
everything to a **draft** GitHub release for you to review and publish.

## What a release contains

| Asset | Built by |
|-------|----------|
| macOS `.dmg` (Apple silicon and Intel) and `.app.tar.gz` | `desktop` job |
| Windows `.msi` and `-setup.exe` | `desktop` job |
| Linux `.AppImage`, `.deb` and `.rpm` | `desktop` job |
| `latest.json` and the `.sig` files used by the in-app updater | `desktop` job |
| `proxyzip-<version>-<target>.tar.gz` / `.zip` (command line, 5 targets) | `cli` job |
| `package-manifests.zip` (Homebrew, Scoop, winget and Flatpak manifests) and `SHA256SUMS` | `packaging` job |

## Signing the installers

Installers are built unsigned unless the repository has these **secrets** (Settings → Secrets and variables → Actions).
The workflow turns signing on by itself when they exist.

| Platform | Secrets |
|----------|---------|
| macOS (signing and notarization) | `APPLE_CERTIFICATE` (base64 of the Developer ID `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), `APPLE_TEAM_ID` |
| Windows | `WINDOWS_CERTIFICATE` (base64 of the code-signing `.pfx`), `WINDOWS_CERTIFICATE_PASSWORD` |

Without signing, macOS shows an “unidentified developer” warning (right-click → Open) and Windows shows SmartScreen.

## Updates

The in-app updater checks `https://github.com/LuisPeixoto/ProxyZip/releases/latest/download/latest.json`, so it only
sees **published** releases. Update packages are signed with a key pair that is independent of OS signing:

- the public key is in `apps/desktop/src-tauri/tauri.conf.json`;
- the private key and its password are the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
  secrets.

**Back the private key up.** If it is lost, installed copies can no longer be updated automatically and users must
reinstall. To rotate it, run `npx tauri signer generate`, replace the secrets and the public key, and ship one release
that people install manually.

## Package managers

The `packaging` job renders the templates in [`packaging/templates`](../packaging/templates) with the release's version
and checksums (`python3 packaging/render.py --version X --assets DIR --out DIR`) and attaches the result to the
release as `package-manifests.zip`. Publishing them is a manual, one-time setup per channel:

| Channel | One-time setup | Each release |
|---------|----------------|--------------|
| Homebrew | Create a tap repository `LuisPeixoto/homebrew-proxyzip` | Copy `homebrew/proxyzip.rb` (cask) and `homebrew/proxyzip-cli.rb` (formula) into it |
| Scoop | Create a bucket repository `LuisPeixoto/scoop-proxyzip` | Copy `scoop/proxyzip.json` (it has `checkver` and `autoupdate`, so Scoop's own tooling can do this) |
| winget | Fork `microsoft/winget-pkgs` | Put the three files of `winget/` under `manifests/p/ProxyZip/ProxyZip/<version>/` and open a pull request |
| Flathub | Follow Flathub's submission process for `dev.proxyzip.app` | Update the manifest's URL and checksum |

Users then install with, for example, `brew install --cask LuisPeixoto/proxyzip/proxyzip`,
`brew install LuisPeixoto/proxyzip/proxyzip-cli`, `scoop bucket add proxyzip https://github.com/LuisPeixoto/scoop-proxyzip`
followed by `scoop install proxyzip`, or `winget install ProxyZip.ProxyZip`.

> The Flatpak manifest repackages the release's `.deb` and has not been built with `flatpak-builder` yet. Test it
> before submitting. The templates are checked in CI (`packaging/test_render.py`), but each channel's own linter
> (`brew audit`, `winget validate`, …) is not run automatically.

## Checklist

1. Update `CHANGELOG.md` and the version in `Cargo.toml`, `apps/desktop/package.json`, `package-lock.json` and
   `tauri.conf.json`.
2. Wait for CI to pass on `main`.
3. `git tag -a vX.Y.Z -m "ProxyZip X.Y.Z" && git push origin vX.Y.Z`.
4. When the workflow finishes, review the draft release and publish it.
