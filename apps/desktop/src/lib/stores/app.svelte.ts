import { listen } from "@tauri-apps/api/event";
import { api, errorText, type FlowEvent, type FlowId, type FlowSummary, type Status } from "../api";

export type Category = "all" | "json" | "html" | "js" | "css" | "img" | "font" | "media" | "ws" | "errors" | "other";
export type SidebarSel =
  | { kind: "all" }
  | { kind: "pinned" }
  | { kind: "errors" }
  | { kind: "ws" }
  | { kind: "host"; value: string }
  | { kind: "client"; value: string };
export type SortKey = "id" | "status" | "method" | "host" | "path" | "type" | "size" | "duration";

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "ok" | "err";
}

function contentCategory(f: FlowSummary): Category {
  if (f.scheme === "ws" || f.scheme === "wss") return "ws";
  const ct = (f.content_type ?? "").toLowerCase();
  if (ct.includes("json")) return "json";
  if (ct.includes("html")) return "html";
  if (ct.includes("javascript") || ct.includes("ecmascript")) return "js";
  if (ct.includes("css")) return "css";
  if (ct.startsWith("image/")) return "img";
  if (ct.startsWith("font/") || ct.includes("woff")) return "font";
  if (ct.startsWith("video/") || ct.startsWith("audio/")) return "media";
  return "other";
}

export function isError(f: FlowSummary): boolean {
  return f.state === "failed" || (f.status ?? 0) >= 400;
}

export function clientName(f: FlowSummary): string {
  const a = f.client_addr;
  if (a === "imported") return "Imported";
  const i = a.lastIndexOf(":");
  const ip = i > 0 ? a.slice(0, i).replace(/^\[|\]$/g, "") : a;
  return ip === "127.0.0.1" || ip === "::1" ? "This computer" : ip;
}

type Pred = (f: FlowSummary) => boolean;

/**
 * Search syntax: free text matches the URL; `method:post`, `status:4xx`,
 * `host:api`, `type:json`, `-text` negates, `/regex/` matches the URL.
 */
export function parseSearch(q: string): Pred {
  const preds: Pred[] = [];
  for (const raw of q.trim().split(/\s+/).filter(Boolean)) {
    const neg = raw.startsWith("-") && raw.length > 1;
    const tok = neg ? raw.slice(1) : raw;
    let p: Pred;
    const m = /^(\w+):(.*)$/.exec(tok);
    if (tok.length > 2 && tok.startsWith("/") && tok.endsWith("/")) {
      let re: RegExp | null = null;
      try {
        re = new RegExp(tok.slice(1, -1), "i");
      } catch {
        re = null;
      }
      p = re ? (f) => re!.test(f.url) : () => true;
    } else if (m) {
      const [, key, value] = m;
      const v = value.toLowerCase();
      switch (key.toLowerCase()) {
        case "method":
          p = (f) => f.method.toLowerCase() === v;
          break;
        case "status":
          p = /^\dxx$/.test(v)
            ? (f) => f.status != null && Math.floor(f.status / 100) === Number(v[0])
            : (f) => String(f.status ?? "") === v;
          break;
        case "host":
        case "domain":
          p = (f) => f.host.toLowerCase().includes(v);
          break;
        case "type":
          p = (f) => (f.content_type ?? "").toLowerCase().includes(v);
          break;
        case "path":
          p = (f) => f.path.toLowerCase().includes(v);
          break;
        default:
          p = (f) => f.url.toLowerCase().includes(tok.toLowerCase());
      }
    } else {
      const t = tok.toLowerCase();
      p = (f) => f.url.toLowerCase().includes(t);
    }
    preds.push(neg ? (f) => !p(f) : p);
  }
  return (f) => preds.every((p) => p(f));
}

function sortValue(f: FlowSummary, key: SortKey): string | number {
  switch (key) {
    case "id":
      return f.id;
    case "status":
      return f.status ?? (f.state === "failed" ? 1000 : -1);
    case "method":
      return f.method;
    case "host":
      return f.host;
    case "path":
      return f.path;
    case "type":
      return f.content_type ?? "";
    case "size":
      return f.response_size ?? -1;
    case "duration":
      return f.duration_ms ?? -1;
  }
}

class AppStore {
  flows = $state.raw<FlowSummary[]>([]);
  #index = new Map<FlowId, number>();

  status = $state<Status | null>(null);
  search = $state("");
  category = $state<Category>("all");
  sidebar = $state<SidebarSel>({ kind: "all" });
  selected = $state.raw<Set<FlowId>>(new Set());
  focused = $state<FlowId | null>(null);
  sort = $state<{ key: SortKey; dir: 1 | -1 }>({ key: "id", dir: 1 });
  theme = $state<"dark" | "light">("dark");
  detailSize = $state(360);
  sidebarOpen = $state(true);
  settingsOpen = $state(false);
  toasts = $state<Toast[]>([]);
  #toastId = 0;

  filtered = $derived.by(() => {
    const searchPred = parseSearch(this.search);
    const cat = this.category;
    const sb = this.sidebar;
    const out = this.flows.filter((f) => {
      if (cat !== "all") {
        if (cat === "errors") {
          if (!isError(f)) return false;
        } else if (contentCategory(f) !== cat) return false;
      }
      switch (sb.kind) {
        case "pinned":
          if (!f.pinned) return false;
          break;
        case "errors":
          if (!isError(f)) return false;
          break;
        case "ws":
          if (f.scheme !== "ws" && f.scheme !== "wss") return false;
          break;
        case "host":
          if (f.host !== sb.value) return false;
          break;
        case "client":
          if (clientName(f) !== sb.value) return false;
          break;
      }
      return searchPred(f);
    });
    const { key, dir } = this.sort;
    if (key !== "id" || dir !== 1) {
      out.sort((a, b) => {
        const va = sortValue(a, key);
        const vb = sortValue(b, key);
        return (va < vb ? -1 : va > vb ? 1 : a.id - b.id) * dir;
      });
    }
    return out;
  });

  hosts = $derived.by(() => {
    const m = new Map<string, { count: number; errors: number; secure: boolean }>();
    for (const f of this.flows) {
      const e = m.get(f.host) ?? { count: 0, errors: 0, secure: false };
      e.count++;
      if (isError(f)) e.errors++;
      if (f.scheme === "https" || f.scheme === "wss") e.secure = true;
      m.set(f.host, e);
    }
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  });

  clients = $derived.by(() => {
    const m = new Map<string, number>();
    for (const f of this.flows) m.set(clientName(f), (m.get(clientName(f)) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  });

  counts = $derived.by(() => {
    let pinned = 0,
      errors = 0,
      ws = 0;
    for (const f of this.flows) {
      if (f.pinned) pinned++;
      if (isError(f)) errors++;
      if (f.scheme === "ws" || f.scheme === "wss") ws++;
    }
    return { all: this.flows.length, pinned, errors, ws };
  });

  constructor() {
    try {
      const t = localStorage.getItem("pz.theme");
      if (t === "light" || t === "dark") this.theme = t;
      const d = Number(localStorage.getItem("pz.detailSize"));
      if (d > 120) this.detailSize = d;
    } catch {
      /* storage unavailable */
    }
  }

  async init() {
    await this.reload();
    await this.refreshStatus();
    await listen<FlowEvent[]>("proxyzip://events", (e) => this.apply(e.payload));
    await listen("proxyzip://resync", () => this.reload());
    setInterval(() => this.refreshStatus(), 5000);
  }

  async reload() {
    const list = await api.getFlows();
    this.#index = new Map(list.map((f, i) => [f.id, i]));
    this.flows = list;
  }

  async refreshStatus() {
    try {
      this.status = await api.getStatus();
    } catch (e) {
      console.error(e);
    }
  }

  apply(events: FlowEvent[]) {
    let list = this.flows;
    let copied = false;
    const ensureCopy = () => {
      if (!copied) {
        list = list.slice();
        copied = true;
      }
    };
    let rebuild = false;
    for (const ev of events) {
      switch (ev.type) {
        case "created": {
          ensureCopy();
          this.#index.set(ev.flow.id, list.length);
          list.push(ev.flow);
          break;
        }
        case "updated": {
          const i = this.#index.get(ev.flow.id);
          if (i !== undefined) {
            ensureCopy();
            list[i] = ev.flow;
          }
          break;
        }
        case "removed": {
          const gone = new Set(ev.ids);
          list = list.filter((f) => !gone.has(f.id));
          copied = true;
          rebuild = true;
          break;
        }
        case "cleared":
          list = [];
          copied = true;
          rebuild = true;
          break;
      }
    }
    if (rebuild) {
      this.#index = new Map(list.map((f, i) => [f.id, i]));
      const alive = new Set(list.map((f) => f.id));
      if (this.focused != null && !alive.has(this.focused)) this.focused = null;
      this.selected = new Set([...this.selected].filter((id) => alive.has(id)));
    }
    if (copied) this.flows = list;
  }

  get(id: FlowId): FlowSummary | undefined {
    const i = this.#index.get(id);
    return i === undefined ? undefined : this.flows[i];
  }

  select(id: FlowId, mode: "replace" | "toggle" | "range" = "replace") {
    if (mode === "toggle") {
      const s = new Set(this.selected);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      this.selected = s;
    } else if (mode === "range" && this.focused != null) {
      const ids = this.filtered.map((f) => f.id);
      const a = ids.indexOf(this.focused);
      const b = ids.indexOf(id);
      if (a >= 0 && b >= 0) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        this.selected = new Set(ids.slice(lo, hi + 1));
        return;
      }
      this.selected = new Set([id]);
    } else {
      this.selected = new Set([id]);
    }
    this.focused = id;
  }

  selectedIds(): FlowId[] {
    if (this.selected.size) return [...this.selected];
    return this.focused != null ? [this.focused] : [];
  }

  setTheme(t: "dark" | "light") {
    this.theme = t;
    try {
      localStorage.setItem("pz.theme", t);
    } catch {
      /* ignore */
    }
  }

  setDetailSize(px: number) {
    this.detailSize = px;
    try {
      localStorage.setItem("pz.detailSize", String(px));
    } catch {
      /* ignore */
    }
  }

  toast(text: string, kind: Toast["kind"] = "info") {
    const id = ++this.#toastId;
    this.toasts = [...this.toasts, { id, text, kind }];
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === "err" ? 6000 : 2800);
  }

  /** Run an action and surface failures as an error toast. */
  async attempt<T>(fn: () => Promise<T>, ok?: string): Promise<T | undefined> {
    try {
      const r = await fn();
      if (ok) this.toast(ok, "ok");
      return r;
    } catch (e) {
      this.toast(errorText(e), "err");
      return undefined;
    }
  }
}

export const app = new AppStore();
