<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api, base64ToText, errorText, formatBytes, textToBase64, type Decision, type FlowDetail } from "../api";
  import { languageForMime } from "../editor";
  import CodeEditor from "./CodeEditor.svelte";
  import HeadersEditor from "./HeadersEditor.svelte";
  import Icon from "./Icon.svelte";

  type Draft = {
    id: number;
    phase: "request" | "response";
    method: string;
    url: string;
    status: number;
    headers: [string, string][];
    /** Body text when the body is text; null keeps binary bodies untouched. */
    text: string | null;
    bodyBase64: string;
    size: number;
  };

  let draft = $state<Draft | null>(null);
  let original = "";
  let busy = $state(false);

  const current = $derived(app.paused[0]);
  const bodyLanguage = $derived(languageForMime(draft?.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1]));

  const snapshot = (d: Draft) => JSON.stringify([d.method, d.url, d.status, d.headers, d.text]);
  const dirty = $derived(draft ? snapshot(draft) !== original : false);

  // Load the details of the flow that is waiting for a decision.
  $effect(() => {
    const id = current?.id;
    if (id == null) {
      draft = null;
      return;
    }
    if (draft?.id === id) return;
    api
      .getFlowDetail(id)
      .then((d) => {
        if (app.paused[0]?.id !== id) return;
        draft = toDraft(d);
        original = snapshot(draft);
      })
      .catch((e) => app.toast(errorText(e), "err"));
  });

  function toDraft(d: FlowDetail): Draft {
    const response = d.summary.state === "paused" && d.response !== null;
    const body = response ? d.response!.body : d.request.body;
    const b64 = body.base64 ?? "";
    return {
      id: d.summary.id,
      phase: response ? "response" : "request",
      method: d.summary.method,
      url: d.summary.url,
      status: d.response?.status ?? 200,
      headers: ((response ? d.response!.headers : d.request.headers) as [string, string][]).map(([k, v]) => [k, v] as [string, string]),
      text: body.size === 0 ? "" : base64ToText(b64),
      bodyBase64: b64,
      size: body.size,
    };
  }

  async function decide(kind: "continue" | "abort" | "all") {
    if (!draft) return;
    busy = true;
    try {
      if (kind === "all") {
        await api.releaseBreakpoints();
      } else {
        const d = draft;
        let decision: Decision = { type: kind };
        if (kind === "continue" && dirty) {
          const body_base64 = d.text !== null ? textToBase64(d.text) : d.bodyBase64;
          const headers = d.headers.filter(([k]) => k.trim() !== "");
          decision =
            d.phase === "request"
              ? { type: "edit_request", method: d.method, url: d.url, headers, body_base64 }
              : { type: "edit_response", status: d.status, headers, body_base64 };
        }
        await api.resolveBreakpoint(d.id, decision);
      }
      draft = null;
    } catch (e) {
      app.toast(errorText(e), "err");
    } finally {
      busy = false;
    }
  }

  function onkey(e: KeyboardEvent) {
    if (!draft || app.breakpointHidden) return;
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") decide("continue");
  }

  const field = "mono h-8 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent";
</script>

<svelte:window onkeydown={onkey} />

{#if draft && !app.breakpointHidden}
  <div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]">
    <div class="shadow-pop flex max-h-full w-full max-w-[760px] flex-col overflow-hidden rounded-2xl border border-warn/40 bg-elevated" role="dialog" aria-modal="true" aria-label="Breakpoint">
      <header class="flex items-center gap-3 border-b border-line px-5 py-3">
        <span class="grid size-8 place-items-center rounded-lg bg-warn/15 text-warn"><Icon name="pause" size={16} /></span>
        <div class="min-w-0 flex-1">
          <h2 class="text-[14px] font-semibold">Paused {draft.phase}</h2>
          <p class="mono truncate text-[11.5px] text-muted">{current?.method} {current?.url}</p>
        </div>
        {#if app.paused.length > 1}<span class="rounded-full bg-warn/15 px-2 py-0.5 text-[11px] font-medium text-warn">{app.paused.length - 1} more waiting</span>{/if}
        <button onclick={() => (app.breakpointHidden = true)} class="rounded-lg px-2.5 py-1 text-[12px] text-muted hover:bg-hover hover:text-fg" title="Keep working; the request stays paused">Hide</button>
      </header>

      <div class="selectable min-h-0 flex-1 space-y-4 overflow-y-auto p-5 text-[12.5px]">
        {#if draft.phase === "request"}
          <div class="flex gap-2">
            <input bind:value={draft.method} aria-label="Method" spellcheck="false" class="{field} w-24 font-semibold uppercase" />
            <input bind:value={draft.url} aria-label="URL" spellcheck="false" class="{field} min-w-0 flex-1" />
          </div>
        {:else}
          <label class="flex items-center gap-2"><span class="text-muted">Status</span><input type="number" min="100" max="599" bind:value={draft.status} class="{field} w-24" /></label>
        {/if}

        <div>
          <div class="mb-1.5 text-[11.5px] font-semibold tracking-wide text-muted uppercase">Headers</div>
          <HeadersEditor bind:headers={draft.headers} />
        </div>

        <div>
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-[11.5px] font-semibold tracking-wide text-muted uppercase">Body</span>
            <span class="mono text-[11px] text-faint">{formatBytes(draft.size)}</span>
          </div>
          {#if draft.text !== null}
            <div class="h-[280px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
              <CodeEditor value={draft.text} onchange={(v) => draft && (draft.text = v)} readOnly={false} language={bodyLanguage} placeholder="No body" />
            </div>
          {:else}
            <p class="rounded-lg border border-dashed border-line px-3 py-4 text-center text-faint">This body is binary, so it is passed through unchanged.</p>
          {/if}
        </div>
      </div>

      <footer class="flex items-center justify-between gap-2 border-t border-line px-5 py-3">
        <button onclick={() => decide("abort")} disabled={busy} class="rounded-lg border border-err/40 px-3 py-1.5 text-[12.5px] font-medium text-err hover:bg-err/10 disabled:opacity-50">Abort</button>
        <div class="flex items-center gap-2">
          {#if app.paused.length > 1}
            <button onclick={() => decide("all")} disabled={busy} class="rounded-lg border border-line px-3 py-1.5 text-[12.5px] hover:bg-hover disabled:opacity-50">Continue all</button>
          {/if}
          <button onclick={() => decide("continue")} disabled={busy} class="flex items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-[12.5px] font-medium text-white hover:opacity-90 disabled:opacity-50">
            <Icon name="play" size={13} /> {dirty ? "Continue with edits" : "Continue"}
          </button>
        </div>
      </footer>
    </div>
  </div>
{/if}
