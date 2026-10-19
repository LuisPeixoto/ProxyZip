<script lang="ts">
  import Self from "./JsonNode.svelte";

  let { name, value, depth = 0, last = true }: { name?: string | number; value: unknown; depth?: number; last?: boolean } = $props();
  const PAGE = 200;
  // svelte-ignore state_referenced_locally
  let open = $state(depth < 2); // initial expansion only
  let limit = $state(PAGE);

  const isArr = $derived(Array.isArray(value));
  const isObj = $derived(value !== null && typeof value === "object");
  const entries = $derived(
    isObj ? (isArr ? (value as unknown[]).map((v, i) => [i, v] as const) : Object.entries(value as object)) : [],
  );
</script>

<div class="leading-[19px]" style="padding-left:{depth ? 16 : 0}px">
  {#if isObj}
    <button class="group inline-flex items-center text-left" onclick={() => (open = !open)}>
      <span class="inline-block w-3 text-[9px] text-faint group-hover:text-fg">{open ? "▼" : "▶"}</span>
      {#if name !== undefined}<span class={typeof name === "number" ? "tok-num opacity-60" : "tok-key"}>{typeof name === "number" ? name : `"${name}"`}</span><span class="tok-punct">:&nbsp;</span>{/if}
      <span class="tok-punct">{isArr ? "[" : "{"}</span>
      {#if !open}
        <span class="mx-1 rounded bg-hover px-1 text-[10.5px] text-muted">{entries.length} {isArr ? "items" : "keys"}</span><span class="tok-punct">{isArr ? "]" : "}"}{last ? "" : ","}</span>
      {/if}
    </button>
    {#if open}
      {#each entries.slice(0, limit) as [k, v], i (k)}
        <Self name={k} value={v} depth={depth + 1} last={i === entries.length - 1} />
      {/each}
      {#if entries.length > limit}
        <button class="ml-4 text-[11px] text-accent hover:underline" onclick={() => (limit += PAGE * 5)}>
          Show {Math.min(PAGE * 5, entries.length - limit)} more of {entries.length - limit}…
        </button>
      {/if}
      <div><span class="inline-block w-3"></span><span class="tok-punct">{isArr ? "]" : "}"}{last ? "" : ","}</span></div>
    {/if}
  {:else}
    <span class="inline-block w-3"></span>
    {#if name !== undefined}<span class={typeof name === "number" ? "tok-num opacity-60" : "tok-key"}>{typeof name === "number" ? name : `"${name}"`}</span><span class="tok-punct">:&nbsp;</span>{/if}
    {#if typeof value === "string"}<span class="tok-str break-all">"{value}"</span>{:else if typeof value === "number"}<span class="tok-num">{value}</span>{:else if typeof value === "boolean"}<span class="tok-bool">{value}</span>{:else}<span class="tok-null">null</span>{/if}<span class="tok-punct">{last ? "" : ","}</span>
  {/if}
</div>
