<script lang="ts">
  import { EditorState } from "@codemirror/state";
  import { EditorView, lineNumbers } from "@codemirror/view";
  import { MergeView } from "@codemirror/merge";
  import { onDestroy } from "svelte";
  import { api, errorText, formatBytes, type FlowDetail, type Headers } from "../api";
  import { app } from "../stores/app.svelte";
  import { editorTheme, languageExtension, type EditorLanguage } from "../editor";
  import Icon from "./Icon.svelte";

  /** Headers that differ on every request and drown out the differences that matter. */
  const VOLATILE = new Set([
    "date", "age", "x-request-id", "request-id", "x-correlation-id", "x-amzn-requestid", "x-amz-request-id", "x-amz-id-2",
    "cf-ray", "traceparent", "tracestate", "x-trace-id", "server-timing",
  ]);

  let details = $state<[FlowDetail, FlowDetail] | null>(null);
  let failure = $state<string | null>(null);
  let part = $state<"request" | "response">("response");
  let ignoreVolatile = $state(true);
  let host: HTMLDivElement | undefined = $state();
  let merge: MergeView | undefined;

  $effect(() => {
    const ids = app.diff;
    if (!ids) return;
    Promise.all([api.getFlowDetail(ids[0]), api.getFlowDetail(ids[1])])
      .then((pair) => (details = pair))
      .catch((e) => (failure = errorText(e)));
  });

  function headerLines(headers: Headers): string {
    return headers
      .filter(([k]) => !(ignoreVolatile && VOLATILE.has(k.toLowerCase())))
      .map(([k, v]) => `${k.toLowerCase()}: ${v}`)
      .sort()
      .join("\n");
  }

  function bodyText(body: FlowDetail["request"]["body"] | undefined): { text: string; language: EditorLanguage } {
    if (!body || body.kind === "empty") return { text: "", language: "text" };
    if (body.pretty) return { text: body.pretty, language: body.kind === "json" ? "json" : "text" };
    if (body.text != null) return { text: body.text, language: body.kind === "xml" ? "xml" : body.kind === "html" ? "html" : "text" };
    return { text: `‹${formatBytes(body.size)} of binary data›`, language: "text" };
  }

  function describe(d: FlowDetail): { text: string; language: EditorLanguage } {
    const res = d.response;
    const head = part === "request" ? `${d.summary.method} ${d.summary.url}` : res ? `${res.http_version} ${res.status} ${res.reason}` : "(no response)";
    const headers = part === "request" ? d.request.headers : (res?.headers ?? []);
    const body = bodyText(part === "request" ? d.request.body : res?.body);
    return { text: `${head}\n\n${headerLines(headers)}\n\n${body.text}`, language: body.language };
  }

  $effect(() => {
    const pair = details;
    // Re-read these so the view is rebuilt when they change.
    void part;
    void ignoreVolatile;
    if (!pair || !host) return;
    merge?.destroy();
    const [a, b] = [describe(pair[0]), describe(pair[1])];
    const language = languageExtension(a.language === "text" ? b.language : a.language);
    const shared = [editorTheme, lineNumbers(), EditorState.readOnly.of(true), EditorView.lineWrapping, language ?? []];
    merge = new MergeView({
      parent: host,
      a: { doc: a.text, extensions: shared },
      b: { doc: b.text, extensions: shared },
      highlightChanges: true,
      gutter: true,
      collapseUnchanged: { margin: 3, minSize: 6 },
    });
  });

  onDestroy(() => merge?.destroy());

  function close() {
    app.diff = null;
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }

  const label = (d: FlowDetail) => `#${d.summary.id} ${d.summary.method} ${d.summary.host}${d.summary.path}`;
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={close}>
  <div class="shadow-pop flex h-full max-h-[780px] w-full max-w-[1240px] flex-col overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Compare requests" tabindex="-1">
    <header class="flex h-12 shrink-0 items-center gap-3 border-b border-line px-4">
      <Icon name="columns" size={16} />
      <h2 class="text-[14px] font-semibold">Compare</h2>
      <div class="ml-3 flex rounded-lg border border-line p-0.5">
        {#each ["request", "response"] as p (p)}
          <button onclick={() => (part = p as "request" | "response")} class="h-6 rounded-md px-3 text-[12px] font-medium capitalize {part === p ? 'bg-accent-soft text-accent' : 'text-muted hover:text-fg'}">{p}</button>
        {/each}
      </div>
      <label class="ml-2 flex items-center gap-1.5 text-[12px] text-muted"><input type="checkbox" bind:checked={ignoreVolatile} class="accent-[var(--accent)]" /> Ignore volatile headers (date, request ids…)</label>
      <div class="flex-1"></div>
      <button onclick={close} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Close"><Icon name="x" /></button>
    </header>
    {#if failure}
      <p class="selectable m-4 rounded-lg bg-err/8 px-3 py-2 text-[12px] text-err">{failure}</p>
    {:else if details}
      <div class="mono grid shrink-0 grid-cols-2 border-b border-line bg-panel-2 text-[11.5px]">
        <div class="truncate border-r border-line px-4 py-1.5" title={label(details[0])}><span class="text-faint">A</span> {label(details[0])}</div>
        <div class="truncate px-4 py-1.5" title={label(details[1])}><span class="text-faint">B</span> {label(details[1])}</div>
      </div>
    {/if}
    <div bind:this={host} class="diff-host selectable min-h-0 flex-1 overflow-auto"></div>
  </div>
</div>

<style>
  .diff-host :global(.cm-mergeView) {
    min-height: 100%;
  }
  .diff-host :global(.cm-mergeViewEditors) {
    min-height: 100%;
  }
  .diff-host :global(.cm-editor) {
    min-height: 100%;
  }
</style>
