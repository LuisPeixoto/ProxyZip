<script lang="ts">
  import { api, errorText, type PluginTabContent } from "../api";
  import { parseMarkdown } from "../markdown";
  import JsonNode from "./JsonNode.svelte";

  let { flowId, tab, fingerprint }: { flowId: number; tab: string; fingerprint: string } = $props();

  let content = $state<PluginTabContent | null>(null);
  let failure = $state<string | null>(null);

  // `plugin:<plugin id>:<tab id>`
  const parts = $derived(tab.split(":"));

  $effect(() => {
    const [, plugin, id] = parts;
    void fingerprint;
    let current = true;
    api
      .pluginTabContent(flowId, plugin, id)
      .then((c) => {
        if (current) {
          content = c;
          failure = null;
        }
      })
      .catch((e) => {
        if (current) failure = errorText(e);
      });
    return () => {
      current = false;
    };
  });
</script>

<div class="selectable p-3 text-[12.5px]">
  {#if failure}
    <p class="rounded-lg bg-err/8 px-3 py-2 text-err">{failure}</p>
  {:else if !content}
    <p class="text-faint">Loading…</p>
  {:else if content.type === "text"}
    <pre class="mono whitespace-pre-wrap break-words">{content.content}</pre>
  {:else if content.type === "json"}
    <div class="mono"><JsonNode value={content.content} expand={4} /></div>
  {:else if content.type === "table"}
    <table class="w-full border-collapse">
      <thead>
        <tr>{#each content.content.columns as c (c)}<th class="border-b border-line px-2 py-1.5 text-left text-[11px] font-semibold tracking-wide text-muted uppercase">{c}</th>{/each}</tr>
      </thead>
      <tbody>
        {#each content.content.rows as row, r (r)}
          <tr class="border-b border-line/60">{#each row as cell, c (c)}<td class="mono px-2 py-1.5">{String(cell ?? "")}</td>{/each}</tr>
        {/each}
      </tbody>
    </table>
  {:else if content.type === "markdown"}
    <div class="grid max-w-[720px] gap-2 leading-relaxed">
      {#each parseMarkdown(String(content.content)) as block, b (b)}
        {#if block.kind === "heading"}
          <div class="font-semibold {block.level === 1 ? 'text-[16px]' : 'text-[13.5px]'}">{#each block.inline as s, k (k)}{s.text}{/each}</div>
        {:else if block.kind === "list"}
          <ul class="list-disc pl-5">{#each block.items as item, k (k)}<li>{#each item as s, n (n)}{#if s.code}<code class="mono rounded bg-hover px-1">{s.text}</code>{:else if s.bold}<b>{s.text}</b>{:else}{s.text}{/if}{/each}</li>{/each}</ul>
        {:else if block.kind === "code"}
          <pre class="mono overflow-auto rounded-lg border border-line bg-panel-2 p-2.5 text-[12px]">{block.text}</pre>
        {:else}
          <p>{#each block.inline as s, n (n)}{#if s.code}<code class="mono rounded bg-hover px-1">{s.text}</code>{:else if s.bold}<b>{s.text}</b>{:else}{s.text}{/if}{/each}</p>
        {/if}
      {/each}
    </div>
  {/if}
</div>
