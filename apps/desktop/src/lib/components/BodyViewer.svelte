<script lang="ts">
  import type { BodyView } from "../api";
  import { formatBytes } from "../api";
  import { copy } from "../actions";
  import CodeView from "./CodeView.svelte";
  import HexView from "./HexView.svelte";
  import Icon from "./Icon.svelte";
  import JsonNode from "./JsonNode.svelte";

  type Mode = "tree" | "pretty" | "text" | "preview" | "hex";
  let { body }: { body: BodyView } = $props();
  let wrap = $state(false);

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
    if (json) m.push("tree");
    if (body.pretty) m.push("pretty");
    if (body.text != null) m.push("text");
    if (body.kind === "image" || body.kind === "html") m.push("preview");
    if (body.base64 != null) m.push("hex");
    return m;
  });
  let chosen = $state<Mode | null>(null);
  const mode = $derived(chosen && modes.includes(chosen) ? chosen : modes[0]);
  const labels: Record<Mode, string> = { tree: "Tree", pretty: "Pretty", text: "Raw", preview: "Preview", hex: "Hex" };

  const imageSrc = $derived(body.kind === "image" && body.base64 ? `data:${body.mime ?? "image/png"};base64,${body.base64}` : "");
</script>

{#if body.kind === "empty"}
  <div class="grid h-full place-items-center p-6 text-[12px] text-faint">No body</div>
{:else}
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-8 shrink-0 items-center gap-1 border-b border-line px-2">
      {#each modes as m (m)}
        <button
          onclick={() => (chosen = m)}
          class="h-6 rounded-md px-2 text-[11.5px] font-medium {mode === m ? 'bg-accent-soft text-accent' : 'text-muted hover:bg-hover hover:text-fg'}"
        >{labels[m]}</button>
      {/each}
      <div class="flex-1"></div>
      <span class="mono truncate text-[10.5px] text-faint" title={body.mime ?? ""}>
        {body.kind.toUpperCase()} · {formatBytes(body.size)}{#if body.encodings.length} ({body.encodings.join("+")} {formatBytes(body.raw_size)}){/if}
      </span>
      {#if mode === "text" || mode === "pretty"}
        <button title="Wrap lines" onclick={() => (wrap = !wrap)} class="ml-1 h-6 rounded-md px-1.5 text-[11px] {wrap ? 'text-accent' : 'text-faint hover:text-fg'}">wrap</button>
      {/if}
      {#if body.text != null}
        <button title="Copy body" onclick={() => copy(mode === "pretty" && body.pretty ? body.pretty : body.text!, "Body copied")} class="grid size-6 place-items-center rounded-md text-faint hover:bg-hover hover:text-fg">
          <Icon name="copy" size={13} />
        </button>
      {/if}
    </div>
    {#if body.truncated || body.decode_error}
      <div class="flex items-center gap-2 border-b border-line bg-warn/10 px-3 py-1.5 text-[11.5px] text-warn">
        <Icon name="alert" size={13} />
        {body.decode_error ? `Could not decode body: ${body.decode_error}` : "Body exceeded the capture limit and was truncated (raise it in Settings)."}
      </div>
    {/if}
    <div class="min-h-0 flex-1 overflow-auto">
      {#if mode === "tree" && json}
        <div class="selectable mono p-3 text-[12px]"><JsonNode value={json.value} /></div>
      {:else if mode === "pretty" && body.pretty}
        <CodeView text={body.pretty} language={body.kind === "json" ? "json" : "text"} {wrap} />
      {:else if mode === "text" && body.text != null}
        <CodeView text={body.text} language={body.kind === "json" ? "json" : "text"} {wrap} />
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
