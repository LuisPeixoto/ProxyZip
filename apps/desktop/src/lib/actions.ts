import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, newMatcher, type FlowId, type FlowSummary, type RuleSet } from "./api";
import { app, type ToolsTab } from "./stores/app.svelte";

export async function copy(text: string, what = "Copied") {
  await app.attempt(() => writeText(text), what);
}

export async function copyCurl(id: FlowId) {
  const cmd = await app.attempt(() => api.copyAsCurl(id));
  if (cmd) await copy(cmd, "cURL command copied");
}

export async function copyResponseBody(id: FlowId) {
  const d = await app.attempt(() => api.getFlowDetail(id));
  const body = d?.response?.body;
  if (!body) return app.toast("No response body", "info");
  if (body.text == null) return app.toast("Body is binary — use the Hex view", "info");
  await copy(body.pretty ?? body.text, "Response body copied");
}

export async function repeat(id: FlowId) {
  await app.attempt(() => api.repeatFlow(id), "Request repeated");
}

export async function togglePin(ids: FlowId[]) {
  const anyUnpinned = ids.some((id) => !app.get(id)?.pinned);
  await app.attempt(() => api.setPinned(ids, anyUnpinned));
}

export async function remove(ids: FlowId[]) {
  if (ids.length) await app.attempt(() => api.removeFlows(ids));
}

export async function exportHar(ids: FlowId[] | null) {
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  const path = await save({
    defaultPath: `proxyzip-${stamp}.har`,
    filters: [{ name: "HTTP Archive", extensions: ["har"] }],
  });
  if (!path) return;
  const n = await app.attempt(() => api.exportHar(path, ids));
  if (n != null) app.toast(`Exported ${n} flow${n === 1 ? "" : "s"}`, "ok");
}

export async function importHar() {
  const path = await open({ multiple: false, filters: [{ name: "HTTP Archive", extensions: ["har", "json"] }] });
  if (!path || Array.isArray(path)) return;
  const n = await app.attempt(() => api.importHar(path));
  if (n != null) app.toast(`Imported ${n} flow${n === 1 ? "" : "s"}`, "ok");
}

export async function excludeHost(host: string, exclude: boolean) {
  await app.attempt(
    () => api.sslToggleHost(host, exclude),
    exclude ? `${host} will be tunnelled without decryption` : `${host} will be decrypted again`,
  );
}

// ---- Quick rules from the context menu --------------------------------------------------------------------------------

/** `host/path` of a request, without scheme or query: a pattern that matches exactly this resource. */
function urlPattern(f: FlowSummary): string {
  return f.url.replace(/^[a-z]+:\/\//i, "").split("?")[0];
}

async function changeRules(mutate: (rules: RuleSet) => void, message: string, tab?: ToolsTab) {
  const rules = await app.attempt(() => api.getRules());
  if (!rules) return;
  mutate(rules);
  const errors = await app.attempt(() => api.saveRules(rules));
  if (!errors) return;
  app.rules = rules;
  for (const e of errors) app.toast(e, "err");
  app.toast(message, "ok");
  if (tab) app.openTools(tab);
}

export function addBreakpointFor(f: FlowSummary) {
  return changeRules((r) => {
    r.tools.breakpoints = true;
    r.breakpoints.push({ name: `Breakpoint · ${f.host}`, enabled: true, matcher: newMatcher(urlPattern(f)), on_request: true, on_response: true });
  }, `Breakpoint set on ${urlPattern(f)}`);
}

export function blockHost(f: FlowSummary) {
  return changeRules((r) => {
    r.tools.block = true;
    r.block.push({ name: `Block · ${f.host}`, enabled: true, matcher: newMatcher(f.host), action: { type: "status", code: 403 } });
  }, `${f.host} is now blocked`);
}

export async function mapLocalFor(f: FlowSummary) {
  const chosen = await open({ multiple: false, title: "Choose the file to answer with" });
  if (typeof chosen !== "string") return;
  await changeRules((r) => {
    r.tools.map_local = true;
    r.map_local.push({
      name: `Map Local · ${f.host}`,
      enabled: true,
      matcher: newMatcher(urlPattern(f)),
      source: { type: "file", path: chosen },
      status: f.status ?? 200,
      headers: [],
      content_type: null,
    });
  }, `${urlPattern(f)} is now answered from disk`);
}

export function mapRemoteFor(f: FlowSummary) {
  return changeRules((r) => {
    r.tools.map_remote = true;
    r.map_remote.push({
      name: `Map Remote · ${f.host}`,
      enabled: true,
      matcher: newMatcher(f.host),
      scheme: null,
      host: f.host,
      port: null,
      path: null,
      keep_host_header: false,
    });
  }, "Choose where these requests should go", "map_remote");
}

export async function editAndRepeat(f: FlowSummary) {
  const detail = await app.attempt(() => api.getFlowDetail(f.id));
  if (!detail) return;
  // The body shown is decoded, so the encoding and length headers no longer describe it.
  const skip = new Set(["content-length", "content-encoding", "host", "connection", "transfer-encoding"]);
  app.openComposer({
    method: f.method,
    url: f.url.replace(/^ws(s?):\/\//, "http$1://"),
    headers: detail.request.headers.filter(([k]) => !skip.has(k.toLowerCase())).map(([k, v]) => [k, v] as [string, string]),
    body_base64: detail.request.body.base64 ?? "",
  });
}

// ---- Sessions and exports ---------------------------------------------------------------------------------------------

const stamp = () => new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");

export async function saveSession(ids: FlowId[] | null = null, redacted = false) {
  const path = await save({
    defaultPath: `proxyzip-${stamp()}${redacted ? "-redacted" : ""}.pzsession`,
    filters: [{ name: "ProxyZip session", extensions: ["pzsession"] }],
  });
  if (!path) return;
  const n = await app.attempt(() => api.saveSession(path, ids, redacted));
  if (n == null) return;
  // A redacted copy is for sharing, so it does not count as saving your work.
  if (!redacted) app.markSaved();
  app.toast(`Saved ${n} request${n === 1 ? "" : "s"}${redacted ? " with secrets removed" : ""}`, "ok");
  await app.refreshRecent();
}

export async function exportWorkspace() {
  const path = await save({
    defaultPath: `proxyzip-${stamp()}.pzworkspace`,
    filters: [{ name: "ProxyZip workspace", extensions: ["pzworkspace"] }],
  });
  if (!path) return;
  const s = await app.attempt(() => api.exportWorkspace(path, true));
  if (s) app.toast(`Saved ${s.rules} rule${s.rules === 1 ? "" : "s"}, ${s.protos} descriptor${s.protos === 1 ? "" : "s"} (credentials hidden)`, "ok");
}

export async function importWorkspace(merge: boolean) {
  const path = await open({ multiple: false, filters: [{ name: "ProxyZip workspace", extensions: ["pzworkspace"] }] });
  if (typeof path !== "string") return;
  const s = await app.attempt(() => api.importWorkspace(path, merge));
  if (!s) return;
  app.rules = await api.getRules();
  app.toast(`${merge ? "Added" : "Loaded"} ${s.rules} rule${s.rules === 1 ? "" : "s"}`, "ok");
}

/** Choose a session file. With traffic already on screen the user is asked whether to replace it or add to it. */
export async function openSession() {
  const path = await open({ multiple: false, filters: [{ name: "ProxyZip session", extensions: ["pzsession"] }] });
  if (!path || Array.isArray(path)) return;
  await requestOpen(path);
}

export async function requestOpen(path: string) {
  if (app.flows.length === 0) await finishOpen(path, false);
  else app.pendingOpen = path;
}

export async function finishOpen(path: string, append: boolean) {
  app.pendingOpen = null;
  const n = await app.attempt(() => api.openSession(path, append));
  if (n == null) {
    await app.attempt(() => api.forgetRecentSession(path));
  } else {
    app.markSaved();
    app.toast(`Opened ${n} request${n === 1 ? "" : "s"}`, "ok");
  }
  await app.refreshRecent();
}

export async function exportCsv(ids: FlowId[] | null) {
  const path = await save({ defaultPath: `proxyzip-${stamp()}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
  if (!path) return;
  const n = await app.attempt(() => api.exportCsv(path, ids));
  if (n != null) app.toast(`Exported ${n} request${n === 1 ? "" : "s"} to CSV`, "ok");
}

export async function exportPostman(ids: FlowId[] | null) {
  const path = await save({ defaultPath: `proxyzip-${stamp()}.postman_collection.json`, filters: [{ name: "Postman collection", extensions: ["json"] }] });
  if (!path) return;
  const n = await app.attempt(() => api.exportPostman(path, ids));
  if (n != null) app.toast(`Exported ${n} request${n === 1 ? "" : "s"} as a Postman collection`, "ok");
}

export function compare(ids: FlowId[]) {
  if (ids.length !== 2) {
    app.toast("Select exactly two requests to compare them", "info");
    return;
  }
  app.diff = [Math.min(...ids), Math.max(...ids)];
}
