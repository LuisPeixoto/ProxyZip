<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api, formatBytes, formatDuration, type FlowDetail, type Headers } from "../api";
  import { copy, copyCurl, editAndRepeat, repeat } from "../actions";
  import { tagClass } from "./Badges";
  import { methodClass, statusClass } from "./Badges";
  import BodyViewer from "./BodyViewer.svelte";
  import CodeView from "./CodeView.svelte";
  import Icon from "./Icon.svelte";
  import KeyValueTable from "./KeyValueTable.svelte";
  import TimingView from "./TimingView.svelte";
  import WsMessages from "./WsMessages.svelte";

  let detail = $state<FlowDetail | null>(null);
  let reqTab = $state("headers");
  let resTab = $state("body");
  let comment = $state("");
  let split = $state(50);

  const summary = $derived(app.focused != null ? app.get(app.focused) : undefined);
  // Re-fetch when the focused flow changes or progresses (state, ws messages, size).
  const fingerprint = $derived(
    summary ? `${summary.id}:${summary.state}:${summary.ws_message_count}:${summary.response_size}:${summary.pinned}` : "",
  );

  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const fp = fingerprint;
    const id = summary?.id;
    clearTimeout(timer);
    if (id == null) {
      detail = null;
      return;
    }
    timer = setTimeout(async () => {
      try {
        const d = await api.getFlowDetail(id);
        if (fp === fingerprint) {
          const switched = detail?.summary.id !== d.summary.id;
          detail = d;
          if (switched) {
            comment = d.comment ?? "";
            if (d.ws_messages.length || d.summary.scheme.startsWith("ws")) resTab = "messages";
            else if (resTab === "messages") resTab = "body";
          }
        }
      } catch {
        detail = null;
      }
    }, detail?.summary.id === id ? 120 : 0);
  });

  function rawRequest(d: FlowDetail): string {
    const r = d.summary;
    const lines = [`${r.method} ${r.path || "/"} ${r.http_version}`, ...d.request.headers.map(([k, v]) => `${k}: ${v}`), ""];
    return lines.join("\n") + "\n" + (d.request.body.text ?? (d.request.body.size ? `‹${formatBytes(d.request.body.size)} binary›` : ""));
  }

  function rawResponse(d: FlowDetail): string {
    const r = d.response!;
    const lines = [`${r.http_version} ${r.status} ${r.reason}`, ...r.headers.map(([k, v]) => `${k}: ${v}`), ""];
    return lines.join("\n") + "\n" + (r.body.text ?? (r.body.size ? `‹${formatBytes(r.body.size)} binary›` : ""));
  }

  function cookiePairs(set: string[]): [string, string][] {
    return set.map((c) => {
      const [first, ...attrs] = c.split(";");
      const [k, ...v] = first.split("=");
      return [k.trim(), [v.join("=").trim(), ...attrs.map((a) => a.trim())].join("  ·  ")];
    });
  }

  async function saveComment() {
    if (!detail) return;
    await app.attempt(() => api.setComment(detail!.summary.id, comment || null), "Comment saved");
  }

  function startDrag(e: PointerEvent) {
    const host = (e.currentTarget as HTMLElement).parentElement!;
    const rect = host.getBoundingClientRect();
    const move = (ev: PointerEvent) => (split = Math.min(80, Math.max(20, ((ev.clientX - rect.left) / rect.width) * 100)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  const count = (h: Headers | unknown[]) => (h.length ? String(h.length) : "");
</script>

{#snippet tabs(list: { id: string; label: string; badge?: string }[], current: string, set: (id: string) => void)}
  <div class="flex h-8 shrink-0 items-center gap-0.5 border-b border-line bg-panel-2 px-1.5">
    {#each list as t (t.id)}
      <button
        onclick={() => set(t.id)}
        class="relative flex h-full items-center gap-1.5 px-2.5 text-[11.5px] font-medium {current === t.id ? 'text-fg' : 'text-muted hover:text-fg'}"
      >
        {t.label}
        {#if t.badge}<span class="mono rounded bg-hover px-1 text-[10px] text-faint">{t.badge}</span>{/if}
        {#if current === t.id}<span class="absolute inset-x-2 bottom-0 h-[2px] rounded-full bg-accent"></span>{/if}
      </button>
    {/each}
  </div>
{/snippet}

{#if !detail}
  <div class="grid h-full place-items-center bg-panel text-[12.5px] text-faint">
    <div class="text-center">
      <Icon name="layers" size={30} class="mx-auto mb-2 opacity-40" />
      Select a request to inspect it
      <div class="mt-1 text-[11px]">↑ ↓ to navigate · ⌘K to filter · right-click for actions</div>
    </div>
  </div>
{:else}
  {@const d = detail}
  {@const s = d.summary}
  <div class="flex h-full min-h-0 flex-col bg-panel">
    <div class="flex h-10 shrink-0 items-center gap-2.5 border-b border-line px-3">
      <span class="mono text-[12px] font-bold {methodClass(s.method)}">{s.method}</span>
      <span class="selectable mono min-w-0 flex-1 truncate text-[12px]" title={s.url}>{s.url}</span>
      {#if s.status != null || s.state === "failed"}
        <span class="mono rounded-md px-2 py-0.5 text-[11px] font-semibold {statusClass(s.status, s.state === 'failed')}">
          {s.state === "failed" ? "Failed" : `${s.status} ${d.response?.reason ?? ""}`}
        </span>
      {/if}
      <span class="mono text-[11px] text-muted">{formatDuration(s.duration_ms)}</span>
      <span class="mono text-[11px] text-muted">{formatBytes(s.response_size)}</span>
      {#each s.tags as tag (tag)}<span class="rounded px-1.5 py-px text-[10.5px] font-medium {tagClass(tag)}">{tag}</span>{/each}
      <span class="mono rounded border border-line px-1.5 py-px text-[10.5px] text-muted">{s.http_version}</span>
      <div class="ml-1 flex items-center gap-0.5">
        <button title="Copy URL" onclick={() => copy(s.url, "URL copied")} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg"><Icon name="copy" size={14} /></button>
        <button title="Copy as cURL" onclick={() => copyCurl(s.id)} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg"><Icon name="terminal" size={14} /></button>
        <button title="Edit & repeat…" disabled={s.method === "CONNECT"} onclick={() => editAndRepeat(s)} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg disabled:opacity-30"><Icon name="edit" size={14} /></button>
        <button title="Repeat (⌘R)" disabled={s.method === "CONNECT"} onclick={() => repeat(s.id)} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg disabled:opacity-30"><Icon name="repeat" size={14} /></button>
      </div>
    </div>

    {#if s.error}
      <div class="selectable flex items-start gap-2 border-b border-line bg-err/8 px-3 py-2 text-[12px] text-err">
        <Icon name="alert" size={14} class="mt-px shrink-0" />
        <span class="break-words">{s.error}</span>
      </div>
    {/if}

    <div class="flex min-h-0 flex-1">
      <section class="flex min-w-0 flex-col" style="width:{split}%">
        {@render tabs(
          [
            { id: "headers", label: "Headers", badge: count(d.request.headers) },
            { id: "query", label: "Query", badge: count(d.request.query) },
            { id: "cookies", label: "Cookies", badge: count(d.request.cookies) },
            { id: "body", label: "Body", badge: d.request.body.size ? formatBytes(d.request.body.size) : "" },
            { id: "raw", label: "Raw" },
          ],
          reqTab,
          (id) => (reqTab = id),
        )}
        <div class="min-h-0 flex-1 overflow-auto">
          {#if reqTab === "headers"}
            <KeyValueTable rows={d.request.headers} empty="No headers" />
          {:else if reqTab === "query"}
            <KeyValueTable rows={d.request.query} empty="No query parameters" />
          {:else if reqTab === "cookies"}
            <KeyValueTable rows={d.request.cookies} empty="No cookies sent" />
          {:else if reqTab === "body"}
            <BodyViewer body={d.request.body} />
          {:else}
            <CodeView text={rawRequest(d)} wrap />
          {/if}
        </div>
      </section>

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div onpointerdown={startDrag} class="w-[5px] shrink-0 cursor-col-resize border-x border-line bg-panel-2 hover:bg-accent/40"></div>

      <section class="flex min-w-0 flex-1 flex-col">
        {@render tabs(
          [
            ...(s.scheme.startsWith("ws") ? [{ id: "messages", label: "Messages", badge: count(d.ws_messages) }] : []),
            { id: "headers", label: "Headers", badge: count(d.response?.headers ?? []) },
            { id: "body", label: "Body", badge: d.response?.body.size ? formatBytes(d.response.body.size) : "" },
            { id: "cookies", label: "Set-Cookie", badge: count(d.response?.set_cookies ?? []) },
            { id: "raw", label: "Raw" },
            { id: "timing", label: "Timing" },
            { id: "notes", label: "Notes", badge: d.comment ? "•" : "" },
          ],
          resTab,
          (id) => (resTab = id),
        )}
        <div class="min-h-0 flex-1 overflow-auto">
          {#if resTab === "timing"}
            <TimingView detail={d} />
          {:else if resTab === "notes"}
            <div class="flex h-full flex-col gap-2 p-3">
              <textarea
                bind:value={comment}
                placeholder="Add a note to this flow (exported with HAR)…"
                class="selectable min-h-[120px] flex-1 resize-none rounded-lg border border-line bg-panel-2 p-3 text-[12.5px] outline-none focus:border-accent"
              ></textarea>
              <button onclick={saveComment} class="self-end rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Save note</button>
            </div>
          {:else if resTab === "messages"}
            <WsMessages messages={d.ws_messages} />
          {:else if !d.response}
            <div class="grid h-full place-items-center text-[12px] text-faint">
              {s.state === "pending" ? "Waiting for response…" : s.state === "tunnel" ? "Encrypted tunnel — not decrypted" : "No response"}
            </div>
          {:else if resTab === "headers"}
            <KeyValueTable rows={d.response.headers} empty="No headers" />
          {:else if resTab === "body"}
            <BodyViewer body={d.response.body} />
          {:else if resTab === "cookies"}
            <KeyValueTable rows={cookiePairs(d.response.set_cookies)} empty="No cookies set" />
          {:else}
            <CodeView text={rawResponse(d)} wrap />
          {/if}
        </div>
      </section>
    </div>
  </div>
{/if}
