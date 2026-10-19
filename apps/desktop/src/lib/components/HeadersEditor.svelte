<script lang="ts">
  import Icon from "./Icon.svelte";

  let { headers = $bindable() }: { headers: [string, string][] } = $props();
  const field = "mono h-7 min-w-0 rounded-md border border-line bg-panel px-2 text-[12px] outline-none placeholder:text-faint focus:border-accent";
</script>

<div class="space-y-1.5">
  {#each headers as header, i (i)}
    <div class="flex items-center gap-1.5">
      <input bind:value={header[0]} placeholder="Header" spellcheck="false" aria-label="Header name" class="{field} w-[34%]" />
      <input bind:value={header[1]} placeholder="Value" spellcheck="false" aria-label="Header value" class="{field} flex-1" />
      <button type="button" onclick={() => headers.splice(i, 1)} title="Remove header" class="grid size-6 shrink-0 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err"><Icon name="x" size={13} /></button>
    </div>
  {/each}
  <button type="button" onclick={() => headers.push(["", ""])} class="flex h-7 items-center gap-1 rounded-md px-1.5 text-[12px] text-accent hover:bg-accent-soft"><Icon name="plus" size={13} /> Add header</button>
</div>
