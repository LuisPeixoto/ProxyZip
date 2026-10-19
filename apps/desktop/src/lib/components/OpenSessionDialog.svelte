<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { finishOpen } from "../actions";
  import Icon from "./Icon.svelte";

  const path = $derived(app.pendingOpen ?? "");
  const name = $derived(path.split(/[\\/]/).pop() ?? path);

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") app.pendingOpen = null;
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={() => (app.pendingOpen = null)}>
  <div class="shadow-pop w-full max-w-[460px] rounded-2xl border border-line bg-elevated p-5" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Open session" tabindex="-1">
    <div class="mb-3 flex items-center gap-3">
      <span class="grid size-9 place-items-center rounded-xl bg-accent-soft text-accent"><Icon name="folderOpen" size={18} /></span>
      <div class="min-w-0">
        <h2 class="text-[14px] font-semibold">Open this session?</h2>
        <p class="mono truncate text-[11.5px] text-muted" title={path}>{name}</p>
      </div>
    </div>
    <p class="mb-5 text-[12.5px] leading-relaxed text-muted">
      There are already {app.flows.length.toLocaleString()} requests in the list. Replace them with the session, or add the session's requests below them.
      {#if app.dirty}<span class="text-warn"> The current list has not been saved.</span>{/if}
    </p>
    <div class="flex justify-end gap-2">
      <button onclick={() => (app.pendingOpen = null)} class="rounded-lg border border-line px-3.5 py-1.5 text-[12.5px] hover:bg-hover">Cancel</button>
      <button onclick={() => finishOpen(path, true)} class="rounded-lg border border-line px-3.5 py-1.5 text-[12.5px] font-medium hover:bg-hover">Add to the list</button>
      <button onclick={() => finishOpen(path, false)} class="rounded-lg bg-accent px-4 py-1.5 text-[12.5px] font-medium text-white hover:opacity-90">Replace the list</button>
    </div>
  </div>
</div>
