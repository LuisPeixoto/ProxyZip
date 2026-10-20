import { invoke } from "@tauri-apps/api/core";

export type FlowId = number;
export type Scheme = "http" | "https" | "ws" | "wss";
export type FlowState = "pending" | "receiving" | "web_socket_open" | "completed" | "failed" | "tunnel" | "paused";

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
  tags: string[];
  graphql: string | null;
  has_logs: boolean;
}

export type Headers = [string, string][];

export type BodyKind =
  | "empty" | "json" | "xml" | "html" | "javascript" | "css" | "form"
  | "multipart" | "text" | "image" | "protobuf" | "binary";

export interface Structured {
  format: "protobuf" | "grpc" | "msgpack" | "sse" | "graphql";
  json: unknown;
  note: string | null;
}

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
  structured: Structured | null;
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
  logs: LogLine[];
}

export interface LogLine {
  level: "log" | "warn" | "error";
  message: string;
  timestamp: number;
}

export type FlowEvent =
  | { type: "created"; flow: FlowSummary }
  | { type: "updated"; flow: FlowSummary }
  | { type: "ws_message"; id: FlowId; message: unknown }
  | { type: "breakpoint_hit"; id: FlowId; phase: "request" | "response" }
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
  listener_errors: string[];
}

export interface Preferences {
  port: number;
  allow_lan: boolean;
  ssl: { intercept_all: boolean; include: string[]; exclude: string[] };
  max_body_capture_mb: number;
  insecure_upstream: boolean;
  max_flows: number;
  restore_last_session: boolean;
  mcp_enabled: boolean;
  mcp_port: number;
  mcp_allow_changes: boolean;
  check_updates: boolean;
  strip_alt_svc: boolean;
  http3_upstream: boolean;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  enabled: boolean;
  contributes: {
    hooks: { pattern: string; request: boolean; response: boolean } | null;
    viewers: { id: string; name: string }[];
    copy_as: { id: string; name: string }[];
  };
}
export interface PluginList {
  plugins: PluginInfo[];
  errors: { folder: string; message: string }[];
  dir: string;
}
export interface McpInfo {
  enabled: boolean;
  port: number;
  running: boolean;
  url: string;
  token: string;
  allow_changes: boolean;
  error: string | null;
}

// ---- Rules -----------------------------------------------------------------------------------------------------------

export interface Matcher {
  methods: string[];
  pattern: string;
  regex: boolean;
}
export type BlockAction = { type: "status"; code: number } | { type: "drop" };
export interface BlockRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  action: BlockAction;
}
export type LocalSource = { type: "file"; path: string } | { type: "directory"; path: string };
export interface MapLocalRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  source: LocalSource;
  status: number;
  headers: [string, string][];
  content_type: string | null;
}
export interface MapRemoteRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  scheme: string | null;
  host: string | null;
  port: number | null;
  path: string | null;
  keep_host_header: boolean;
}
export type RewriteOp =
  | { type: "set_header"; name: string; value: string }
  | { type: "add_header"; name: string; value: string }
  | { type: "remove_header"; name: string }
  | { type: "set_query"; name: string; value: string }
  | { type: "remove_query"; name: string }
  | { type: "set_status"; status: number }
  | { type: "replace_body"; find: string; replace: string; regex: boolean };
export interface RewriteRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  phase: "request" | "response";
  ops: RewriteOp[];
}
export interface BreakpointRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  on_request: boolean;
  on_response: boolean;
}
export interface ThrottleProfile {
  name: string;
  latency_ms: number;
  download_kbps: number;
  upload_kbps: number;
}
export interface DnsRule {
  name: string;
  enabled: boolean;
  host: string;
  ip: string | null;
}
export interface UpstreamProxy {
  enabled: boolean;
  kind: "http" | "socks5";
  host: string;
  port: number;
  username: string | null;
  password: string | null;
  bypass: string[];
}
export interface ReverseRule {
  name: string;
  enabled: boolean;
  listen_port: number;
  target: string;
  h3?: boolean;
}
export interface RuleSet {
  tools: {
    block: boolean;
    map_local: boolean;
    map_remote: boolean;
    rewrite: boolean;
    breakpoints: boolean;
    scripts: boolean;
    dns: boolean;
    reverse: boolean;
  };
  block: BlockRule[];
  record_only: { enabled: boolean; matchers: Matcher[] };
  map_local: MapLocalRule[];
  map_remote: MapRemoteRule[];
  rewrite: RewriteRule[];
  breakpoints: BreakpointRule[];
  scripts: ScriptRule[];
  proto_mappings: ProtoMapping[];
  throttle: { enabled: boolean; profile: ThrottleProfile; matchers: Matcher[] };
  dns: DnsRule[];
  upstream: UpstreamProxy;
  reverse: ReverseRule[];
  socks: { enabled: boolean; port: number };
}

export interface ScriptRule {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  source: string;
  on_request: boolean;
  on_response: boolean;
}
export interface ProtoMapping {
  name: string;
  enabled: boolean;
  matcher: Matcher;
  request_type: string | null;
  response_type: string | null;
}
export interface ProtoInfo {
  files: string[];
  messages: string[];
  methods: string[];
}
export interface JavaInfo {
  found: boolean;
  truststore: string | null;
  store_type: string | null;
  password: string;
  options: string | null;
}
export interface FirefoxReport {
  profiles: string[];
  imported: number;
  notes: string[];
}
export interface IosInfo {
  proxy: string | null;
  page: string | null;
  qr_svg: string | null;
}
export interface AndroidDevice {
  serial: string;
  state: string;
  model: string | null;
  emulator: boolean;
}
export interface LanguageInfo {
  id: string;
  label: string;
  syntax: string;
}
export interface RecentSession {
  path: string;
  name: string;
  saved_at: number;
  flows: number;
}
export interface ScriptPhaseResult {
  outcome: "unchanged" | "changed" | "block" | "error";
  error?: string;
  value: unknown;
}
export interface ScriptTestResult {
  request: ScriptPhaseResult;
  response: ScriptPhaseResult | null;
  logs: LogLine[];
}

export interface ComposeInput {
  method: string;
  url: string;
  headers: [string, string][];
  body_base64: string;
}

export type Decision =
  | { type: "continue" }
  | { type: "abort" }
  | { type: "edit_request"; method: string; url: string; headers: [string, string][]; body_base64: string }
  | { type: "edit_response"; status: number; headers: [string, string][]; body_base64: string };

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
  terminalEnv: (shell?: string) => invoke<string>("terminal_env", { shell: shell ?? null }),
  listPlugins: () => invoke<PluginList>("list_plugins"),
  installPlugin: (path: string) => invoke<PluginList>("install_plugin", { path }),
  setPluginEnabled: (id: string, enabled: boolean) => invoke<PluginList>("set_plugin_enabled", { id, enabled }),
  removePlugin: (id: string) => invoke<PluginList>("remove_plugin", { id }),
  getMcpInfo: () => invoke<McpInfo>("get_mcp_info"),
  setMcp: (enabled: boolean, port: number, allowChanges: boolean) => invoke<McpInfo>("set_mcp", { enabled, port, allowChanges }),
  regenerateMcpToken: () => invoke<McpInfo>("regenerate_mcp_token"),
  openTerminal: () => invoke<void>("open_terminal"),
  javaSetup: () => invoke<JavaInfo>("java_setup"),
  installFirefox: () => invoke<FirefoxReport>("install_firefox"),
  iosSetup: () => invoke<IosInfo>("ios_setup"),
  androidDevices: () => invoke<AndroidDevice[]>("android_devices"),
  androidSetProxy: (serial: string, enable: boolean) => invoke<void>("android_set_proxy", { serial, enable }),
  androidInstallCert: (serial: string, system: boolean) => invoke<string>("android_install_cert", { serial, system }),
  repeatFlow: (id: FlowId) => invoke<void>("repeat_flow", { id }),
  sendRequest: (request: ComposeInput) => invoke<number>("send_request", { request }),
  parseCurl: (command: string) => invoke<ComposeInput>("parse_curl", { command }),
  getRules: () => invoke<RuleSet>("get_rules"),
  saveRules: (rules: RuleSet) => invoke<string[]>("save_rules", { rules }),
  throttlePresets: () => invoke<ThrottleProfile[]>("throttle_presets"),
  resolveBreakpoint: (id: FlowId, decision: Decision) => invoke<boolean>("resolve_breakpoint", { id, decision }),
  releaseBreakpoints: () => invoke<void>("release_breakpoints"),
  listProtos: () => invoke<ProtoInfo>("list_protos"),
  importProto: (path: string) => invoke<ProtoInfo>("import_proto", { path }),
  removeProto: (name: string) => invoke<ProtoInfo>("remove_proto", { name }),
  codeLanguages: () => invoke<LanguageInfo[]>("code_languages"),
  generateCode: (id: FlowId, language: string) => invoke<string>("generate_code", { id, language }),
  queryBody: (id: FlowId, part: "request" | "response", expr: string) => invoke<unknown[]>("query_body", { id, part, expr }),
  saveSession: (path: string, ids: FlowId[] | null) => invoke<number>("save_session", { path, ids }),
  openSession: (path: string, append: boolean) => invoke<number>("open_session", { path, append }),
  recentSessions: () => invoke<RecentSession[]>("recent_sessions"),
  forgetRecentSession: (path: string) => invoke<void>("forget_recent_session", { path }),
  exportCsv: (path: string, ids: FlowId[] | null) => invoke<number>("export_csv", { path, ids }),
  exportPostman: (path: string, ids: FlowId[] | null) => invoke<number>("export_postman", { path, ids }),
  testScript: (source: string, id: FlowId) => invoke<ScriptTestResult>("test_script", { source, id }),
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

/** Encode text as base64 (UTF-8). */
export function textToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary);
}

/** Decode base64 into text, or null when the bytes are not valid UTF-8. */
export function base64ToText(b64: string): string | null {
  try {
    const binary = atob(b64);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

export function emptyRules(): RuleSet {
  return {
    tools: { block: true, map_local: true, map_remote: true, rewrite: true, breakpoints: true, scripts: true, dns: true, reverse: true },
    block: [],
    record_only: { enabled: false, matchers: [] },
    map_local: [],
    map_remote: [],
    rewrite: [],
    breakpoints: [],
    scripts: [],
    proto_mappings: [],
    throttle: { enabled: false, profile: { name: "Custom", latency_ms: 0, download_kbps: 0, upload_kbps: 0 }, matchers: [] },
    dns: [],
    upstream: { enabled: false, kind: "http", host: "", port: 8080, username: null, password: null, bypass: [] },
    reverse: [],
    socks: { enabled: false, port: 9394 },
  };
}

export function newMatcher(pattern = ""): Matcher {
  return { methods: [], pattern, regex: false };
}
