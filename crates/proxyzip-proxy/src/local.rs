//! Pages served by the proxy itself (certificate download, status).

use std::sync::Arc;

use hyper::Response;

use crate::server::Ctx;
use crate::util::{ProxyBody, format_authority, full, split_authority};

/// Browse to `http://proxyzip.cert` through the proxy to download the root CA.
pub const CERT_HOST: &str = "proxyzip.cert";

pub(crate) fn is_local(host: &str, port: u16, ctx: &Ctx) -> bool {
    let host = host.to_ascii_lowercase();
    if host == CERT_HOST {
        return true;
    }
    port == ctx.listen.port() && matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
}

/// `host_header` is the `Host` the client used to reach us, so a PAC file points back at the same address.
pub(crate) fn respond(path: &str, ctx: &Arc<Ctx>, host_header: Option<&str>) -> Response<ProxyBody> {
    match path.split('?').next().unwrap_or("/") {
        "/ca.pem" | "/proxyzip-ca.pem" => Response::builder()
            .header("content-type", "application/x-pem-file")
            .header("content-disposition", "attachment; filename=\"proxyzip-ca.pem\"")
            .body(full(ctx.ca.cert_pem().to_string()))
            .unwrap(),
        "/ca.crt" | "/ca.cer" => Response::builder()
            .header("content-type", "application/x-x509-ca-cert")
            .header("content-disposition", "attachment; filename=\"proxyzip-ca.crt\"")
            .body(full(ctx.ca.cert_der().to_vec()))
            .unwrap(),
        "/ca.mobileconfig" => Response::builder()
            .header("content-type", "application/x-apple-aspen-config")
            .header("content-disposition", "attachment; filename=\"ProxyZip.mobileconfig\"")
            .body(full(proxyzip_certinstall::ios::mobileconfig(ctx.ca.cert_der().as_ref())))
            .unwrap(),
        "/proxy.pac" | "/proxy.pac.js" | "/wpad.dat" => Response::builder()
            .header("content-type", "application/x-ns-proxy-autoconfig")
            .body(full(pac_script(ctx, host_header)))
            .unwrap(),
        _ => Response::builder().header("content-type", "text/html; charset=utf-8").body(full(PAGE)).unwrap(),
    }
}

fn pac_script(ctx: &Ctx, host_header: Option<&str>) -> String {
    let from_header = host_header
        .map(|h| split_authority(h, ctx.listen.port()).0)
        .filter(|h| !h.is_empty() && !h.eq_ignore_ascii_case(CERT_HOST));
    let host = from_header.unwrap_or_else(|| {
        if ctx.listen.ip().is_unspecified() { "127.0.0.1".to_string() } else { ctx.listen.ip().to_string() }
    });
    let address = format_authority(&host, ctx.listen.port());
    format!(
        "function FindProxyForURL(url, host) {{\n  if (isPlainHostName(host) || shExpMatch(host, \"*.local\") || host === \"127.0.0.1\") return \"DIRECT\";\n  return \"PROXY {address}; DIRECT\";\n}}\n"
    )
}

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>ProxyZip</title><style>
body{font:16px/1.5 system-ui,sans-serif;background:#0b0d12;color:#e6e8ee;margin:0;display:grid;place-items:center;min-height:100vh}
main{max-width:520px;padding:32px}h1{font-size:28px;margin:0 0 8px}p{color:#9aa3b2}
a.btn{display:inline-block;margin:8px 8px 0 0;padding:12px 18px;border-radius:10px;background:#6d5efc;color:#fff;text-decoration:none;font-weight:600}
a.alt{background:#1c2130}code{background:#1c2130;padding:2px 6px;border-radius:6px}
</style></head><body><main><h1>⚡ ProxyZip</h1>
<p>Your traffic is flowing through ProxyZip. Install the root certificate on this device to inspect HTTPS.</p>
<a class="btn" href="/ca.crt">Download certificate (.crt)</a><a class="btn alt" href="/ca.mobileconfig">iOS profile</a><a class="btn alt" href="/ca.pem">PEM</a>
<p><b>iOS:</b> tap <i>iOS profile</i>, allow it, install it in Settings → Profile Downloaded, then Settings → General → About → Certificate Trust Settings → enable full trust.<br>
<b>Android:</b> Settings → Security → Encryption &amp; credentials → Install a certificate → CA certificate.</p>
</main></body></html>"#;
