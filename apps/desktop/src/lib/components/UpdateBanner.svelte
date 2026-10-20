<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { installUpdate } from "../updater";
  import Icon from "./Icon.svelte";

  let dismissed = $state<string | null>(null);
  const installing = $derived(app.updateProgress !== undefined);
</script>

{#if app.update && dismissed !== app.update.version}
  <div class="shadow-pop fixed bottom-4 left-4 z-30 flex max-w-[420px] items-center gap-3 rounded-xl border border-line bg-elevated px-4 py-3 text-[12.5px]" role="status">
    <span class="grid size-8 shrink-0 place-items-center rounded-lg bg-accent-soft text-accent"><Icon name="zap" size={16} /></span>
    <div class="min-w-0 flex-1">
      <div class="font-medium">ProxyZip {app.update.version} is available</div>
      {#if installing}
        <div class="mt-1.5 h-1.5 overflow-hidden rounded-full bg-hover"><div class="h-full rounded-full bg-accent transition-all" style:width="{app.updateProgress ?? 100}%"></div></div>
        <div class="mt-1 text-[11px] text-muted">Downloading… the app restarts when it is done.</div>
      {:else}
        <div class="text-[11.5px] text-muted">Install it now or keep working.</div>
      {/if}
    </div>
    {#if !installing}
      <button onclick={installUpdate} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Install & restart</button>
      <button onclick={() => (dismissed = app.update?.version ?? null)} class="grid size-6 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Later"><Icon name="x" size={14} /></button>
    {/if}
  </div>
{/if}
