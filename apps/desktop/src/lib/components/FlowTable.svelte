<script lang="ts">
  import { app, type SortKey } from "../stores/app.svelte";
  import { api, formatBytes, formatDuration, formatTime, type FlowSummary } from "../api";
  import { methodClass, statusClass, tagClass, typeLabel } from "./Badges";
  import {
    addBreakpointFor,
    blockHost,
    compare,
    copy,
    copyCurl,
    copyResponseBody,
    editAndRepeat,
    excludeHost,
    exportHar,
    mapLocalFor,
    mapRemoteFor,
    remove,
    repeat,
    togglePin,
  } from "../actions";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";
  import Onboarding from "./Onboarding.svelte";

  const ROW = 28;
  const OVERSCAN = 12;
  const COLS = "52px 62px 74px minmax(150px,1.1fr) minmax(220px,3fr) 76px 78px 78px 108px";

  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let height = $state(400);
  let stickToBottom = $state(true);
  let menu: { x: number; y: number; items: MenuItem[] } | null = $state(null);

  const rows = $derived(app.filtered);
  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const end = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const visible = $derived(rows.slice(start, end));
  const sortedById = $derived(app.sort.key === "id" && app.sort.dir === 1);

  // Follow new traffic while the user sits at the bottom, like a terminal.
  $effect(() => {
    const n = rows.length;
    if (viewport && stickToBottom && sortedById) {
      queueMicrotask(() => viewport && (viewport.scrollTop = n * ROW));
    }
  });

  function onscroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    stickToBottom = viewport.scrollTop + viewport.clientHeight >= viewport.scrollHeight - ROW * 2;
  }

  function setSort(key: SortKey) {
    app.sort = app.sort.key === key ? { key, dir: app.sort.dir === 1 ? -1 : 1 } : { key, dir: key === "id" ? 1 : -1 };
  }

  function click(e: MouseEvent, f: FlowSummary) {
    app.select(f.id, e.shiftKey ? "range" : e.metaKey || e.ctrlKey ? "toggle" : "replace");
  }

  function ensureVisible(index: number) {
    if (!viewport) return;
    const top = index * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = top + ROW - viewport.clientHeight;
  }

  function onkeydown(e: KeyboardEvent) {
    if (!rows.length) return;
    const idx = app.focused == null ? -1 : rows.findIndex((r) => r.id === app.focused);
    let next = idx;
    if (e.key === "ArrowDown") next = Math.min(rows.length - 1, idx + 1);
    else if (e.key === "ArrowUp") next = Math.max(0, idx < 0 ? 0 : idx - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = rows.length - 1;
    else if (e.key === "PageDown") next = Math.min(rows.length - 1, idx + Math.floor(height / ROW));
    else if (e.key === "PageUp") next = Math.max(0, idx - Math.floor(height / ROW));
    else if ((e.key === "Backspace" || e.key === "Delete") && (e.metaKey || e.key === "Delete")) {
      e.preventDefault();
      remove(app.selectedIds());
      return;
    } else if (e.key === "a" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      app.selected = new Set(rows.map((r) => r.id));
      return;
    } else return;
    e.preventDefault();
    app.select(rows[next].id, e.shiftKey ? "range" : "replace");
    ensureVisible(next);
  }

  async function makeMock(id: number) {
    const count = await app.attempt(() => api.addMockFromFlow(id));
    if (count == null) return;
    app.rules = await api.getRules();
    app.toast(`Mocked. The “Captured” server now answers ${count} route${count === 1 ? "" : "s"}.`, "ok");
  }

  async function addToSuite(ids: number[]) {
    const suite = await app.attempt(() => api.suiteFromFlows(ids));
    if (!suite) return;
    app.suiteDraft = suite;
    app.openTools("suites");
  }

  function contextmenu(e: MouseEvent, f: FlowSummary) {
    e.preventDefault();
    if (!app.selected.has(f.id)) app.select(f.id);
    const ids = app.selectedIds();
    const many = ids.length > 1;
    menu = {
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: "Copy URL", icon: "copy", action: () => copy(f.url, "URL copied") },
        { label: "Copy as cURL", icon: "terminal", disabled: many, action: () => copyCurl(f.id) },
        { label: "Generate code…", icon: "code", disabled: many || f.method === "CONNECT", action: () => (app.codeFor = f.id) },
        { label: "Create a mock from this response", icon: "server", disabled: many || f.method === "CONNECT" || f.status == null, action: () => makeMock(f.id) },
        { label: many ? `Add ${ids.length} requests to a suite…` : "Add to a suite…", icon: "play", disabled: ids.every((id) => app.get(id)?.method === "CONNECT"), action: () => addToSuite(ids) },
        { label: "Compare the two selected", icon: "columns", disabled: ids.length !== 2, shortcut: "⌘D", action: () => compare(ids) },
        { label: "Copy response body", icon: "code", disabled: many || f.response_size == null, action: () => copyResponseBody(f.id) },
        { separator: true },
        { label: "Repeat request", icon: "repeat", shortcut: "⌘R", disabled: many || f.method === "CONNECT", action: () => repeat(f.id) },
        { label: "Edit & repeat…", icon: "edit", disabled: many || f.method === "CONNECT", action: () => editAndRepeat(f) },
        { label: f.pinned ? "Unpin" : "Pin", icon: "pin", action: () => togglePin(ids) },
        { label: many ? `Export ${ids.length} flows as HAR…` : "Export as HAR…", icon: "download", action: () => exportHar(ids) },
        { separator: true },
        { label: `Only show ${f.host}`, icon: "filter", action: () => (app.sidebar = { kind: "host", value: f.host }) },
        { separator: true },
        { label: "Set breakpoint", icon: "pause", disabled: many || f.method === "CONNECT", action: () => addBreakpointFor(f) },
        { label: "Map Local…", icon: "file", disabled: many || f.method === "CONNECT", action: () => mapLocalFor(f) },
        { label: "Map Remote…", icon: "shuffle", disabled: many || f.method === "CONNECT", action: () => mapRemoteFor(f) },
        { label: `Block ${f.host}`, icon: "ban", disabled: many, action: () => blockHost(f) },
        { label: `Don't decrypt ${f.host}`, icon: "unlock", action: () => excludeHost(f.host, true) },
        { separator: true },
        { label: many ? `Delete ${ids.length} flows` : "Delete", icon: "trash", shortcut: "⌘⌫", danger: true, action: () => remove(ids) },
      ],
    };
  }

  const headers: { key: SortKey; label: string; align?: string }[] = [
    { key: "id", label: "#" },
    { key: "status", label: "Status" },
    { key: "method", label: "Method" },
    { key: "host", label: "Host" },
    { key: "path", label: "Path" },
    { key: "type", label: "Type" },
    { key: "size", label: "Size", align: "text-right" },
    { key: "duration", label: "Duration", align: "text-right" },
    { key: "id", label: "Time", align: "text-right" },
  ];

  function statusText(f: FlowSummary): string {
    if (f.state === "failed") return "ERR";
    if (f.state === "paused") return "PAUSED";
    if (f.state === "tunnel") return "TUN";
    if (f.status != null) return String(f.status);
    return "···";
  }
</script>

<div class="flex min-h-0 flex-1 flex-col bg-panel">
  <div class="grid h-7 shrink-0 items-center border-b border-line bg-panel-2 text-[11px] font-medium text-muted" style="grid-template-columns:{COLS}">
    {#each headers as h, i (i)}
      <button onclick={() => setSort(h.key)} class="flex h-full items-center gap-1 truncate px-2.5 hover:text-fg {h.align === 'text-right' ? 'justify-end' : ''}">
        {h.label}
        {#if app.sort.key === h.key && !(h.key === "id" && app.sort.dir === 1) && (i !== 8)}
          <Icon name={app.sort.dir === 1 ? "arrowUp" : "arrowDown"} size={11} />
        {/if}
      </button>
    {/each}
  </div>

  {#if !app.flows.length}
    <Onboarding />
  {:else if !rows.length}
    <div class="grid flex-1 place-items-center text-[13px] text-faint">
      <div class="text-center">
        <Icon name="filter" size={28} class="mx-auto mb-2 opacity-50" />
        No flows match the current filters
      </div>
    </div>
  {:else}
    <div
      bind:this={viewport}
      bind:clientHeight={height}
      {onscroll}
      {onkeydown}
      tabindex="0"
      role="grid"
      aria-rowcount={rows.length}
      class="relative min-h-0 flex-1 overflow-auto outline-none"
    >
      <div style="height:{rows.length * ROW}px" class="relative">
        {#each visible as f, i (f.id)}
          {@const failed = f.state === "failed"}
          {@const selected = app.selected.has(f.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            role="row"
            tabindex="-1"
            aria-selected={selected}
            onclick={(e) => click(e, f)}
            oncontextmenu={(e) => contextmenu(e, f)}
            class="absolute inset-x-0 grid items-center border-b border-line/50 text-[12px] {selected
              ? 'bg-select'
              : (start + i) % 2
                ? 'bg-panel-2/40 hover:bg-hover'
                : 'hover:bg-hover'} {Date.now() - f.created_at < 1200 ? 'row-new' : ''} {failed ? 'text-err' : ''}"
            style="top:{(start + i) * ROW}px; height:{ROW}px; grid-template-columns:{COLS}"
          >
            <span class="mono truncate px-2.5 text-[11px] text-faint">
              {#if f.pinned}<Icon name="pin" size={11} class="inline text-accent" />{:else}{f.id}{/if}
            </span>
            <span class="px-2">
              <span class="mono inline-flex h-[18px] min-w-[38px] items-center justify-center rounded-[5px] px-1.5 text-[11px] font-semibold {f.state === 'paused' ? 'bg-warn/15 text-warn' : statusClass(f.status, failed)}">
                {statusText(f)}
              </span>
            </span>
            <span class="mono truncate px-2.5 text-[11px] font-semibold {methodClass(f.method)}">
              {f.scheme === "ws" || f.scheme === "wss" ? "WS" : f.method}
            </span>
            <span class="flex items-center gap-1.5 truncate px-2.5">
              {#if f.scheme === "https" || f.scheme === "wss"}<Icon name="lock" size={11} class="shrink-0 text-faint" />{/if}
              <span class="truncate">{f.host}{f.port !== 443 && f.port !== 80 ? `:${f.port}` : ""}</span>
            </span>
            <span class="mono truncate px-2.5 text-[11.5px] {failed ? '' : 'text-fg/80'}" title={failed ? f.error : f.url}>
              {#if failed && f.error}<span class="opacity-90">{f.path || f.host} — {f.error}</span>{:else}{f.path}{/if}
              {#if f.graphql}<span class="ml-1.5 rounded bg-accent-soft px-1 text-[10px] font-medium text-accent">{f.graphql}</span>{/if}
              {#each f.tags as tag (tag)}<span class="ml-1.5 rounded px-1 text-[10px] font-medium {tagClass(tag)}">{tag}</span>{/each}
              {#if f.ws_message_count}<span class="ml-1.5 rounded bg-accent-soft px-1 text-[10px] text-accent">{f.ws_message_count} msgs</span>{/if}
              {#if f.has_comment}<Icon name="comment" size={11} class="ml-1 inline text-warn" />{/if}
            </span>
            <span class="truncate px-2.5 text-[11px] text-muted">{typeLabel(f.content_type)}</span>
            <span class="mono truncate px-2.5 text-right text-[11px] text-muted">{formatBytes(f.response_size)}</span>
            <span class="mono truncate px-2.5 text-right text-[11px] {f.duration_ms != null && f.duration_ms > 1000 ? 'text-warn' : 'text-muted'}">
              {f.state === "pending" || f.state === "receiving" ? "…" : formatDuration(f.duration_ms)}
            </span>
            <span class="mono truncate px-2.5 text-right text-[11px] text-faint">{formatTime(f.created_at)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}
