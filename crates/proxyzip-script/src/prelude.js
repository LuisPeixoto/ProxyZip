// Runs once per worker runtime. Natives (`__log`, `__md5`, …) are installed from Rust before this executes.
(function (g) {
  const format = (v) => {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.stack || String(v);
    try {
      const s = JSON.stringify(v);
      return s === undefined ? String(v) : s;
    } catch (_) {
      return String(v);
    }
  };
  const logger = (level) => (...args) => __log(level, args.map(format).join(" "));
  g.console = { log: logger("log"), info: logger("log"), debug: logger("log"), warn: logger("warn"), error: logger("error") };

  const decodeUrlSafe = (part) => JSON.parse(__b64dec(String(part).replace(/-/g, "+").replace(/_/g, "/")));
  g.$ = {
    base64: { encode: (s) => __b64enc(String(s)), decode: (s) => __b64dec(String(s)) },
    md5: (s) => __md5(String(s)),
    sha1: (s) => __sha1(String(s)),
    sha256: (s) => __sha256(String(s)),
    hmacSha256: (key, message) => __hmac256(String(key), String(message)),
    uuid: () => __uuid(),
    json: { parse: (s) => JSON.parse(s), stringify: (v, space) => JSON.stringify(v, null, space) },
    jwt: {
      decode(token) {
        const parts = String(token).split(".");
        if (parts.length < 2) throw new Error("not a JWT: expected at least two dot-separated parts");
        return { header: decodeUrlSafe(parts[0]), payload: decodeUrlSafe(parts[1]), signature: parts[2] || "" };
      },
    },
    sleep: (ms) => __sleep(Number(ms) || 0),
  };

  g.__scripts = {};
  g.__register = (id, factory) => {
    g.__scripts[id] = factory();
  };
  g.__run = (id, phase, sharedJson, requestJson, responseJson) => {
    const script = g.__scripts[id];
    const ctx = { shared: JSON.parse(sharedJson) };
    const request = JSON.parse(requestJson);
    const response = responseJson ? JSON.parse(responseJson) : undefined;
    if (phase === "decode" || phase === "generate") {
      // Plugin entry points: `decode(body, info)` returns {format, json}; `generate(request)` returns text.
      const entry = script[phase];
      if (!entry) return JSON.stringify({ skipped: true, shared: ctx.shared });
      const value = phase === "decode" ? entry(request, response) : entry(request);
      if (value && typeof value.then === "function") throw new Error(`${phase} must return a value, not a Promise`);
      return JSON.stringify({ result: value === undefined ? null : value, shared: ctx.shared });
    }
    const fn = phase === "request" ? script.onRequest : script.onResponse;
    if (!fn) return JSON.stringify({ skipped: true, shared: ctx.shared });
    const out = phase === "request" ? fn(ctx, request) : fn(ctx, request, response);
    if (out && typeof out.then === "function") {
      throw new Error(`${phase === "request" ? "onRequest" : "onResponse"} must return a value, not a Promise: async functions are not supported`);
    }
    const original = phase === "request" ? request : response;
    // Returning nothing means "use the object I was given, with my edits".
    return JSON.stringify({ result: out === undefined ? original : out, shared: ctx.shared, blocked: out === null && phase === "request" });
  };
})(globalThis);
