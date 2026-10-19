<script lang="ts">
  import { api, errorText, formatBytes, type BodyView } from "../api";
  import { app } from "../stores/app.svelte";
  import { copy } from "../actions";
  import CodeEditor from "./CodeEditor.svelte";
  import CodeView from "./CodeView.svelte";
  import HexView from "./HexView.svelte";
  import Icon from "./Icon.svelte";
  import JsonNode from "./JsonNode.svelte";

  type Mode = "decoded" | "tree" | "pretty" | "text" | "preview" | "hex";
  let { body, flowId, part }: { body: BodyView; flowId?: number; part?: "request" | "response" } = $props();
  let wrap = $state(false);
  let decodedAsJson = $state(false);

  const json = $derived.by(() => {
    if (body.kind !== "json" || body.text == null) return undefined;
    try {
      return { value: JSON.parse(body.text) as unknown };
    } catch {
      return undefined;
    }
  });

  const modes = $derived.by((): Mode[] => {
    const m: Mode[] = [];
    if (body.structured) m.push("decoded");
    if (json) m.push("tree");
    if (body.pretty) m.push("pretty");
    if (body.text != null) m.push("text");
    if (body.kind === "image" || body.kind === "html") m.push("preview");
    if (body.base64 != null) m.push("hex");
    return m;
  });
  let chosen = $state<Mode | null>(null);
  const mode = $derived(chosen && modes.includes(chosen) ? chosen : modes[0]);
  const labels: Record<Mode, string> = { decoded: "Decoded", tree: "Tree", pretty: "Pretty", text: "Raw", preview: "Preview", hex: "Hex" };
  const formatNames: Record<string, string> = { protobuf: "Protobuf", grpc: "gRPC", msgpack: "MessagePack", sse: "Event stream", graphql: "GraphQL" };

  const imageSrc = $derived(body.kind === "image" && body.base64 ? `data:${body.mime ?? "image/png"};base64,${body.base64}` : "");
  const decodedText = $derived(body.structured ? JSON.stringify(body.structured.json, null, 2) : "");

  // ---- jq filter -------------------------------------------------------------------------------------------------------
  let jqOpen = $state(false);
  let jqExpr = $state("");
  let jqOutput = $state<{ text: string; count: number } | null>(null);
  let jqError = $state<string | null>(null);
  let jqBusy = $state(false);
  const HISTORY_KEY = "pz.jqHistory";

  function history(): string[] {
    try {
      return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]") as string[];
    } catch {
      return [];
    }
  }
  let jqHistory = $state<string[]>(history());

  async function runJq() {
    if (flowId == null || !part || !jqExpr.trim()) return;
    jqBusy = true;
    jqError = null;
    try {
      const results = await api.queryBody(flowId, part, jqExpr);
      jqOutput = { count: results.length, text: results.map((r) => (typeof r === "string" ? JSON.stringify(r) : JSON.stringify(r, null, 2))).join("\n") };
      jqHistory = [jqExpr, ...jqHistory.filter((h) => h !== jqExpr)].slice(0, 20);
      try {
        localStorage.setItem(HISTORY_KEY, JSON.stringify(jqHistory));
      } catch {
        /* storage unavailable */
      }
    } catch (e) {
      jqOutput = null;
      jqError = errorText(e);
    } finally {
      jqBusy = false;
    }
  }

  function clearJq() {
    jqOutput = null;
    jqError = null;
  }
</script>

{#if body.kind === "empty"}
  <div class="grid h-full place-items-center p-6 text-[12px] text-faint">No body</div>
{:else}
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-8 shrink-0 items-center gap-1 border-b border-line px-2">
      {#each modes as m (m)}
        <button
          onclick={() => {
            chosen = m;
            clearJq();
          }}
          class="h-6 rounded-md px-2 text-[11.5px] font-medium {mode === m && !jqOutput && !jqError ? 'bg-accent-soft text-accent' : 'text-muted hover:bg-hover hover:text-fg'}"
        >{labels[m]}</button>
      {/each}
      {#if json && flowId != null}
        <button
          onclick={() => (jqOpen = !jqOpen)}
          title="Filter this JSON with jq"
          class="ml-1 h-6 rounded-md px-2 text-[11.5px] font-medium {jqOpen || jqOutput ? 'bg-accent-soft text-accent' : 'text-muted hover:bg-hover hover:text-fg'}"
        >jq</button>
      {/if}
      <div class="flex-1"></div>
      <span class="mono truncate text-[10.5px] text-faint" title={body.mime ?? ""}>
        {body.kind.toUpperCase()} · {formatBytes(body.size)}{#if body.encodings.length} ({body.encodings.join("+")} {formatBytes(body.raw_size)}){/if}
      </span>
      {#if mode === "text" || mode === "pretty" || mode === "decoded"}
        <button title="Wrap lines" onclick={() => (wrap = !wrap)} class="ml-1 h-6 rounded-md px-1.5 text-[11px] {wrap ? 'text-accent' : 'text-faint hover:text-fg'}">wrap</button>
      {/if}
      {#if body.text != null}
        <button title="Copy body" onclick={() => copy(mode === "decoded" ? decodedText : mode === "pretty" && body.pretty ? body.pretty : body.text!, "Body copied")} class="grid size-6 place-items-center rounded-md text-faint hover:bg-hover hover:text-fg">
          <Icon name="copy" size={13} />
        </button>
      {/if}
    </div>

    {#if jqOpen && json && flowId != null}
      <form
        class="flex shrink-0 items-center gap-2 border-b border-line bg-panel-2 px-2 py-1.5"
        onsubmit={(e) => {
          e.preventDefault();
          runJq();
        }}
      >
        <span class="mono text-[11px] text-faint">jq</span>
        <input
          bind:value={jqExpr}
          spellcheck="false"
          placeholder={'.items[] | select(.price > 10) | .name'}
          aria-label="jq filter"
          class="mono h-7 min-w-0 flex-1 rounded-md border border-line bg-panel px-2 text-[12px] outline-none placeholder:text-faint focus:border-accent"
        />
        {#if jqHistory.length}
          <select
            aria-label="jq history"
            title="Previous filters"
            class="h-7 w-7 shrink-0 cursor-pointer appearance-none rounded-md border border-line bg-panel text-center text-[11px] text-muted outline-none hover:text-fg"
            onchange={(e) => {
              const v = e.currentTarget.value;
              if (v) jqExpr = v;
              e.currentTarget.value = "";
            }}
          >
            <option value="">⌄</option>
            {#each jqHistory as h (h)}<option value={h}>{h}</option>{/each}
          </select>
        {/if}
        <button type="submit" disabled={jqBusy || !jqExpr.trim()} class="h-7 rounded-md bg-accent px-2.5 text-[11.5px] font-medium text-white hover:opacity-90 disabled:opacity-50">Run</button>
        {#if jqOutput || jqError}<button type="button" onclick={clearJq} class="h-7 rounded-md px-2 text-[11.5px] text-muted hover:bg-hover">Clear</button>{/if}
      </form>
    {/if}

    {#if body.truncated || body.decode_error}
      <div class="flex items-center gap-2 border-b border-line bg-warn/10 px-3 py-1.5 text-[11.5px] text-warn">
        <Icon name="alert" size={13} />
        {body.decode_error ? `Could not decode body: ${body.decode_error}` : "Body exceeded the capture limit and was truncated (raise it in Settings)."}
      </div>
    {/if}

    {#if jqError}
      <div class="selectable mono border-b border-line bg-err/8 px-3 py-2 text-[12px] break-words text-err">{jqError}</div>
    {/if}

    <div class="min-h-0 flex-1 overflow-auto">
      {#if jqOutput}
        <div class="flex h-full flex-col">
          <div class="shrink-0 border-b border-line px-3 py-1 text-[11px] text-muted">{jqOutput.count} result{jqOutput.count === 1 ? "" : "s"}</div>
          <div class="min-h-0 flex-1"><CodeEditor value={jqOutput.text} language="json" {wrap} readOnly /></div>
        </div>
      {:else if mode === "decoded" && body.structured}
        <div class="flex h-full flex-col">
          <div class="flex shrink-0 items-center gap-2 border-b border-line bg-panel-2 px-3 py-1.5 text-[11.5px]">
            <span class="rounded bg-accent-soft px-1.5 py-px font-semibold text-accent">{formatNames[body.structured.format] ?? body.structured.format}</span>
            {#if body.structured.note}<span class="min-w-0 flex-1 truncate text-muted" title={body.structured.note}>{body.structured.note}</span>{:else}<span class="flex-1"></span>{/if}
            <button onclick={() => (decodedAsJson = !decodedAsJson)} class="rounded-md px-1.5 text-[11px] text-muted hover:text-fg">{decodedAsJson ? "Tree" : "JSON"}</button>
          </div>
          <div class="min-h-0 flex-1 overflow-auto">
            {#if decodedAsJson}
              <CodeEditor value={decodedText} language="json" {wrap} readOnly />
            {:else}
              <div class="selectable mono p-3 text-[12px]"><JsonNode value={body.structured.json} expand={4} /></div>
            {/if}
          </div>
        </div>
      {:else if mode === "tree" && json}
        <div class="selectable mono p-3 text-[12px]"><JsonNode value={json.value} /></div>
      {:else if mode === "pretty" && body.pretty}
        <CodeView text={body.pretty} language={body.kind === "json" ? "json" : "text"} {wrap} />
      {:else if mode === "text" && body.text != null}
        <CodeView text={body.text} language={body.kind === "json" ? "json" : body.kind === "xml" ? "xml" : body.kind === "html" ? "html" : body.kind === "javascript" ? "javascript" : body.kind === "css" ? "css" : "text"} {wrap} />
      {:else if mode === "preview" && body.kind === "image"}
        <div class="grid h-full place-items-center bg-[repeating-conic-gradient(var(--hover)_0%_25%,transparent_0%_50%)] bg-[length:16px_16px] p-4">
          <img src={imageSrc} alt="Response preview" class="max-h-full max-w-full object-contain shadow-lg" />
        </div>
      {:else if mode === "preview" && body.kind === "html"}
        <iframe title="HTML preview" sandbox="" srcdoc={body.text ?? ""} class="h-full w-full bg-white"></iframe>
      {:else if mode === "hex" && body.base64 != null}
        <HexView base64={body.base64} />
      {:else}
        <p class="p-4 text-[12px] text-faint">This body is too large to display ({formatBytes(body.size)}).</p>
      {/if}
    </div>
  </div>
{/if}
