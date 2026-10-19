<script lang="ts">
  import type { WsView } from "../api";
  import { formatBytes, formatTime } from "../api";
  import CodeView from "./CodeView.svelte";
  import HexView from "./HexView.svelte";
  import Icon from "./Icon.svelte";

  let { messages }: { messages: WsView[] } = $props();
  let selected = $state<number | null>(null);
  let q = $state("");
  const shown = $derived(
    messages.map((m, i) => ({ m, i })).filter(({ m }) => !q || (m.text ?? "").toLowerCase().includes(q.toLowerCase())),
  );
  const current = $derived(selected != null ? messages[selected] : null);

  function pretty(text: string): string {
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="border-b border-line px-3 py-1.5">
    <input bind:value={q} placeholder="Filter {messages.length} messages…" class="h-6 w-full rounded-md border border-line bg-panel-2 px-2 text-[12px] outline-none focus:border-accent" />
  </div>
  <div class="min-h-0 flex-1 overflow-auto {current ? 'max-h-[45%] border-b border-line' : ''}">
    {#each shown as { m, i } (i)}
      <button
        onclick={() => (selected = selected === i ? null : i)}
        class="flex h-7 w-full items-center gap-2 border-b border-line/50 px-3 text-left text-[12px] {selected === i ? 'bg-select' : 'hover:bg-hover'}"
      >
        <Icon name={m.direction === "outgoing" ? "arrowUp" : "arrowDown"} size={13} class={m.direction === "outgoing" ? "text-ok" : "text-info"} />
        <span class="mono w-12 shrink-0 text-[10.5px] text-faint uppercase">{m.kind}</span>
        <span class="mono min-w-0 flex-1 truncate text-[11.5px]">{m.text ?? `‹${formatBytes(m.size)} binary›`}</span>
        <span class="mono shrink-0 text-[10.5px] text-faint">{formatBytes(m.size)}</span>
        <span class="mono shrink-0 text-[10.5px] text-faint">{formatTime(m.timestamp)}</span>
      </button>
    {:else}
      <p class="p-4 text-[12px] text-faint">No messages yet.</p>
    {/each}
  </div>
  {#if current}
    <div class="min-h-0 flex-1 overflow-auto">
      {#if current.text != null}
        <CodeView text={pretty(current.text)} language="json" wrap />
      {:else}
        <HexView base64={current.base64} />
      {/if}
    </div>
  {/if}
</div>
