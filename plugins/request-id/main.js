// Hook: runs for every request that matches `hooks` in plugin.json.
function onRequest(ctx, request) {
  const has = Object.keys(request.headers).some((name) => name.toLowerCase() === "x-request-id");
  if (!has) {
    request.headers["x-request-id"] = $.uuid();
  }
  return request;
}
