// ProxyZip: route Node.js http/https traffic through the ProxyZip proxy.
// Loaded with `--require` (see `proxyzip env`). Node 24+ can do this natively with NODE_USE_ENV_PROXY=1;
// this file covers older versions. It honours HTTP_PROXY, HTTPS_PROXY and NO_PROXY.
"use strict";
if (!globalThis.__proxyzipPreload) {
  globalThis.__proxyzipPreload = true;
  const http = require("http");
  const https = require("https");
  const tls = require("tls");
  const proxyFor = (secure) => {
    const raw = secure
      ? process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || process.env.http_proxy
      : process.env.HTTP_PROXY || process.env.http_proxy;
    if (!raw) return null;
    try {
      return new URL(raw);
    } catch (_) {
      return null;
    }
  };
  const bypass = (host) => {
    const list = (process.env.NO_PROXY || process.env.no_proxy || "localhost,127.0.0.1,::1")
      .split(",")
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean);
    host = String(host || "").toLowerCase();
    return list.some((p) => p === "*" || host === p || host.endsWith(p.startsWith(".") ? p : "." + p));
  };
  const basicAuth = (proxy) =>
    proxy.username
      ? "Basic " + Buffer.from(decodeURIComponent(proxy.username) + ":" + decodeURIComponent(proxy.password)).toString("base64")
      : null;

  class ConnectAgent extends https.Agent {
    createConnection(options, callback) {
      const proxy = proxyFor(true);
      const host = options.hostname || options.host;
      if (!proxy || bypass(host)) return super.createConnection(options, callback);
      const headers = { host: host + ":" + (options.port || 443) };
      const auth = basicAuth(proxy);
      if (auth) headers["proxy-authorization"] = auth;
      const req = http.request({
        host: proxy.hostname,
        port: proxy.port || 80,
        method: "CONNECT",
        path: host + ":" + (options.port || 443),
        headers,
        agent: false,
      });
      req.once("connect", (res, socket) => {
        if (res.statusCode !== 200) {
          socket.destroy();
          return callback(new Error("proxy refused CONNECT: " + res.statusCode));
        }
        const secure = tls.connect({ ...options, socket, servername: options.servername || host });
        callback(null, secure);
      });
      req.once("error", callback);
      req.end();
    }
  }
  https.globalAgent = new ConnectAgent({ keepAlive: true });

  const originalRequest = http.request;
  const patched = function (input, options, cb) {
    let opts;
    if (typeof input === "string" || input instanceof URL) {
      const u = new URL(input);
      opts = { protocol: u.protocol, hostname: u.hostname, port: u.port, path: u.pathname + u.search, ...(typeof options === "object" ? options : {}) };
      cb = typeof options === "function" ? options : cb;
    } else {
      opts = { ...input };
      cb = typeof options === "function" ? options : cb;
    }
    const proxy = proxyFor(false);
    const host = opts.hostname || opts.host || "localhost";
    if (!proxy || opts.agent || opts.socketPath || bypass(host) || (opts.protocol && opts.protocol !== "http:")) {
      return originalRequest.call(http, input, options, cb);
    }
    const port = opts.port || 80;
    const target = "http://" + host + (String(port) === "80" ? "" : ":" + port) + (opts.path || "/");
    const headers = { ...(opts.headers || {}) };
    if (!headers.host && !headers.Host) headers.host = host + (String(port) === "80" ? "" : ":" + port);
    const auth = basicAuth(proxy);
    if (auth) headers["proxy-authorization"] = auth;
    return originalRequest.call(http, { ...opts, hostname: proxy.hostname, host: proxy.hostname, port: proxy.port || 80, path: target, headers }, cb);
  };
  http.request = patched;
  http.get = function (...args) {
    const req = patched(...args);
    req.end();
    return req;
  };
}
