<script lang="ts">
  import { copy } from "../actions";
  import Icon from "./Icon.svelte";

  let { rows, empty = "Nothing here" }: { rows: [string, string][]; empty?: string } = $props();
  let q = $state("");
  const shown = $derived(
    q ? rows.filter(([k, v]) => k.toLowerCase().includes(q.toLowerCase()) || v.toLowerCase().includes(q.toLowerCase())) : rows,
  );
</script>

{#if !rows.length}
  <p class="p-4 text-[12px] text-faint">{empty}</p>
{:else}
  {#if rows.length > 8}
    <div class="sticky top-0 z-10 border-b border-line bg-panel px-3 py-1.5">
      <input bind:value={q} placeholder="Filter…" class="h-6 w-full rounded-md border border-line bg-panel-2 px-2 text-[12px] outline-none focus:border-accent" />
    </div>
  {/if}
  <table class="w-full border-collapse text-[12px]">
    <tbody>
      {#each shown as [k, v], i (i)}
        <tr class="group border-b border-line/60 align-top hover:bg-hover/60">
          <td class="selectable mono w-[34%] max-w-[260px] px-3 py-[5px] font-medium break-all text-accent">{k}</td>
          <td class="selectable mono px-3 py-[5px] break-all text-fg/90">{v}</td>
          <td class="w-7 pr-2">
            <button title="Copy value" onclick={() => copy(v)} class="invisible grid size-5 place-items-center rounded text-faint group-hover:visible hover:text-fg">
              <Icon name="copy" size={12} />
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
