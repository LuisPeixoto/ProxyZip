# Certificates

## How HTTPS interception works

When an application opens a secure connection through ProxyZip, ProxyZip:

1. Reads the host name the client asked for (the TLS *server name*).
2. Creates a certificate for that host, signed by the ProxyZip root CA.
3. Completes the TLS handshake with the client using that certificate.
4. Opens its own, fully verified TLS connection to the real server.
5. Forwards the traffic between the two while recording it.

The client only accepts step 3 if it trusts the ProxyZip root CA. That is why you install it once.

## Where the CA lives

| Platform | Directory |
|----------|-----------|
| macOS | `~/Library/Application Support/ProxyZip` |
| Windows | `%APPDATA%\ProxyZip` |
| Linux | `~/.local/share/ProxyZip` |

Set `PROXYZIP_HOME` to use a different directory. It contains:

- `proxyzip-ca.pem`: the public certificate, safe to share.
- `proxyzip-ca-key.pem`: the **private key**. Keep it secret. It is written with owner-only permissions on Unix.

`proxyzip ca path` prints the certificate path.

## Installing the CA

The desktop app and the CLI do the same thing.

| Platform | What happens | CLI |
|----------|--------------|-----|
| macOS | Added to your login keychain and trusted for TLS | `proxyzip ca install` |
| Windows | Added to the current user's *Trusted Root Certification Authorities* | `proxyzip ca install` |
| Linux | Copied to the system trust anchors (asks for permission through `pkexec`) and, when present, added to the user's NSS database used by Chrome and Chromium | `proxyzip ca install` |
| iOS Simulator | Installed into the booted simulator | `proxyzip ca ios-simulator` |

Check the current state with `proxyzip ca status`. Remove the CA with `proxyzip ca uninstall`.

### Other places that need their own setup

- **Firefox** keeps its own certificate store. Import `proxyzip-ca.pem` under
  *Settings → Privacy & Security → Certificates → View Certificates → Authorities → Import*,
  or enable `security.enterprise_roots.enabled` in `about:config`.
- **Node.js, Python, Go, Ruby, curl** read the CA from environment variables. `eval "$(proxyzip env)"` sets them.
- **Java** uses its own keystore. Import the certificate with `keytool -importcert`.
- **Physical phones and tablets:** open `http://proxyzip.cert` through the proxy. See [Getting started](getting-started.md).

## Certificate pinning

Some applications only accept specific certificates (*pinning*), so they reject the ProxyZip CA no matter what the
system trusts. ProxyZip shows these as failed `CONNECT` requests with a TLS error.

You can stop decrypting those hosts so the application keeps working:

- Right-click the request and choose **Don't decrypt `<host>`**, or
- Add the host to **Settings → SSL proxying → Never decrypt** (wildcards such as `*.example.com` work).

Excluded hosts are tunnelled untouched, so you still see that a connection happened, just not its contents.

## Rotating the CA

**Settings → Certificate → Regenerate** (or `proxyzip ca regenerate`) creates a new CA. Remove the old one from your
trust stores first, then install the new one.

## Security notes

- Anyone with the private key can impersonate any website to a device that trusts the CA. Do not copy it to other
  machines, and uninstall the CA from devices you no longer test with.
- The CA has a path length of zero, so it cannot issue intermediate CAs. Leaf certificates are valid for
  one year.
