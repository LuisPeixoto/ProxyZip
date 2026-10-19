# Getting started

This guide takes you from installation to your first captured request.

## 1. Start ProxyZip

Open the desktop app. The proxy starts automatically on `127.0.0.1:9393`. If that port is busy, ProxyZip tries the
next ten ports and shows the one it picked in the toolbar. You can change the port in **Settings → Proxy**.

Prefer a terminal? Run `proxyzip start` instead. See the [command-line reference](cli.md).

## 2. Trust the certificate

HTTPS traffic is encrypted end to end. To read it, ProxyZip presents a certificate signed by its own certificate
authority (CA), and your system has to trust that CA.

- Click **Install certificate** in the toolbar (or **Settings → Certificate → Install & trust**).
- Your operating system asks for permission. Approve it.

The toolbar now says **HTTPS ready**. Details for every platform are in [Certificates](certificates.md).

> The CA is generated on your machine the first time ProxyZip runs. Its private key is stored in your user data
> directory with owner-only permissions and is never uploaded anywhere.

## 3. Send traffic through ProxyZip

Pick whichever fits what you are debugging.

### The whole computer

Click **System proxy** in the toolbar. Browsers and most applications honour the system proxy. ProxyZip restores
your previous setting when you quit.

### One terminal

```bash
eval "$(proxyzip env)"
curl https://example.com
```

This exports `HTTP_PROXY`, `HTTPS_PROXY` and the CA bundle variables used by curl, Node.js, Python, Go and others.
In the desktop app, **Copy shell setup** puts the same snippet on your clipboard.

### A single command

```bash
curl -x http://127.0.0.1:9393 --cacert "$(proxyzip ca path)" https://example.com
```

### A phone, tablet or emulator

1. In **Settings → Proxy**, enable **Allow LAN connections**.
2. On the device, open the Wi-Fi settings and set an HTTP proxy to your computer's address and port 9393.
   The first-run screen shows your address.
3. On the device, open `http://proxyzip.cert` and install the certificate.
   - **iOS:** install the downloaded profile, then enable it under
     *Settings → General → About → Certificate Trust Settings*.
   - **Android:** *Settings → Security → Encryption & credentials → Install a certificate → CA certificate*.
     Note that Android 7 and newer apps only trust user certificates if they opt in.
4. The iOS Simulator on macOS can be set up in one click with **Settings → Certificate → iOS Simulator**.

## 4. Inspect

- Click a request to open the inspector. The left side is the request, the right side is the response.
- Use the filter bar to narrow down traffic. See [Filtering](filtering.md).
- Right-click a request for actions: copy as cURL, repeat, pin, export, or stop decrypting that host.

## 5. Save your work

- **Export HAR** in the toolbar writes a standard HTTP Archive that you can open in browser dev tools or share.
- **Import** loads a HAR file back into the list.

## Next steps

- [Certificates](certificates.md)
- [Filtering](filtering.md)
- [Troubleshooting](troubleshooting.md)
