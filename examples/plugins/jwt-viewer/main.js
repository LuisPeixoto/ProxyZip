// Viewer: `decode(body, info)` returns { format, json } to show the body in the "Decoded" view.
function decode(body, info) {
  const token = (body.text || "").trim();
  const jwt = $.jwt.decode(token);
  const exp = jwt.payload.exp;
  return {
    format: "jwt",
    json: {
      header: jwt.header,
      payload: jwt.payload,
      expires: exp ? new Date(exp * 1000).toISOString() : null,
    },
  };
}

// Copy as: `generate(request)` returns text that appears as another tab in the Code window.
function generate(request) {
  const headers = Object.entries(request.headers)
    .map(([name, value]) => `${name}: ${value}`)
    .join("\n");
  return ["```http", `${request.method} ${request.url}`, headers, "```"].join("\n");
}
