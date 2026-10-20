// Hook: runs for every request. The settings decide what it does.
function onRequest(ctx, request) {
  const { host, name, value, only_when_missing } = plugin.settings;
  // QuickJS has no URL class, so pick the host out of the address by hand.
  const authority = (/^[a-z][a-z0-9+.-]*:\/\/([^\/?#]*)/i.exec(request.url) || [])[1] || "";
  if (host && !authority.toLowerCase().includes(host.toLowerCase())) return request;
  if (!name) return request;
  const exists = Object.keys(request.headers).some((h) => h.toLowerCase() === name.toLowerCase());
  if (exists && only_when_missing) return request;
  request.headers[name] = value;
  return request;
}
