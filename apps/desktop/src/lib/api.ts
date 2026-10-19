import { invoke } from "@tauri-apps/api/core";

export type FlowId = number;
export type Scheme = "http" | "https" | "ws" | "wss";
export type FlowState = "pending" | "receiving" | "web_socket_open" | "completed" | "failed" | "tunnel";

export interface FlowSummary {
  id: FlowId;
  created_at: number;
  method: string;
  scheme: Scheme;
  host: string;
  port: number;
  path: string;
  url: string;
  http_version: string;
  status: number | null;
  content_type: string | null;
  request_size: number;
  response_size: number | null;
  duration_ms: number | null;
  state: FlowState;
  error: string | null;
  client_addr: string;
  ws_message_count: number;
  pinned: boolean;
  has_comment: boolean;
}

export type Headers = [string, string][];

export type BodyKind =
  | "empty" | "json" | "xml" | "html" | "javascript" | "css" | "form"
  | "multipart" | "text" | "image" | "protobuf" | "binary";

export interface BodyView {
  kind: BodyKind;
  mime: string | null;
  encodings: string[];
  decode_error: string | null;
  raw_size: number;
  size: number;
  text: string | null;
  pretty: string | null;
  base64: string | null;
  truncated: boolean;
}

export interface Timing {
  started: number;
  request_sent: number | null;
  response_started: number | null;
  finished: number | null;
}

export interface TlsInfo {
  sni: string | null;
  alpn: string | null;
  version: string | null;
  cipher: string | null;
}

export interface WsView {
  direction: "outgoing" | "incoming";
  kind: "text" | "binary" | "ping" | "pong" | "close";
  timestamp: number;
  size: number;
  text: string | null;
  base64: string;
}

export interface FlowDetail {
  summary: FlowSummary;
  request: { headers: Headers; query: [string, string][]; cookies: [string, string][]; body: BodyView };
  response: {
    status: number;
    reason: string;
    http_version: string;
    headers: Headers;
    set_cookies: string[];
    body: BodyView;
  } | null;
  timing: Timing;
  tls: TlsInfo | null;
  comment: string | null;
  ws_messages: WsView[];
}

export type FlowEvent =
  | { type: "created"; flow: FlowSummary }
  | { type: "updated"; flow: FlowSummary }
  | { type: "ws_message"; id: FlowId; message: unknown }
  | { type: "removed"; ids: FlowId[] }
  | { type: "cleared" };

export interface Status {
  running: boolean;
  host: string | null;
  port: number | null;
  listen: string | null;
  recording: boolean;
  error: string | null;
  system_proxy: boolean;
  ca_installed: boolean;
  ca_detail: string;
  ca_path: string;
  lan_addresses: string[];
  platform: string;
}

export interface Preferences {
  port: number;
  allow_lan: boolean;
  ssl: { intercept_all: boolean; include: string[]; exclude: string[] };
  max_body_capture_mb: number;
  insecure_upstream: boolean;
  max_flows: number;
}

export const api = {
  getFlows: () => invoke<FlowSummary[]>("get_flows"),
  getFlowDetail: (id: FlowId) => invoke<FlowDetail>("get_flow_detail", { id }),
  clearFlows: () => invoke<void>("clear_flows"),
  removeFlows: (ids: FlowId[]) => invoke<void>("remove_flows", { ids }),
  setPinned: (ids: FlowId[], pinned: boolean) => invoke<void>("set_pinned", { ids, pinned }),
  setComment: (id: FlowId, comment: string | null) => invoke<void>("set_comment", { id, comment }),
  getStatus: () => invoke<Status>("get_status"),
  setRecording: (recording: boolean) => invoke<void>("set_recording", { recording }),
  restartProxy: () => invoke<string>("restart_proxy"),
  setSystemProxy: (enabled: boolean) => invoke<void>("set_system_proxy", { enabled }),
  installCa: () => invoke<void>("install_ca"),
  uninstallCa: () => invoke<void>("uninstall_ca"),
  regenerateCa: () => invoke<void>("regenerate_ca"),
  installCaIosSimulator: () => invoke<void>("install_ca_ios_simulator"),
  getSettings: () => invoke<Preferences>("get_settings"),
  updateSettings: (prefs: Preferences) => invoke<void>("update_settings", { prefs }),
  copyAsCurl: (id: FlowId) => invoke<string>("copy_as_curl", { id }),
  exportHar: (path: string, ids: FlowId[] | null) => invoke<number>("export_har", { path, ids }),
  importHar: (path: string) => invoke<number>("import_har", { path }),
  terminalEnv: () => invoke<string>("terminal_env"),
  repeatFlow: (id: FlowId) => invoke<void>("repeat_flow", { id }),
  sslToggleHost: (host: string, exclude: boolean) => invoke<void>("ssl_proxying_toggle_host", { host, exclude }),
};

export function formatBytes(n: number | null | undefined): string {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "";
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(2)} s`;
  return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
}

export function formatTime(ts: number): string {
  const d = new Date(ts);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
