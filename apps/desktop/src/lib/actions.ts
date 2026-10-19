import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type FlowId } from "./api";
import { app } from "./stores/app.svelte";

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
