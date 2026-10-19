<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "../Icon.svelte";
  import Switch from "./Switch.svelte";

  let {
    name = $bindable(""),
    enabled = $bindable(true),
    ondelete,
    children,
    placeholder = "Untitled rule",
  }: { name?: string; enabled?: boolean; ondelete: () => void; children: Snippet; placeholder?: string } = $props();
</script>

<section class="rounded-xl border border-line bg-panel-2 transition {enabled ? '' : 'opacity-60'}">
  <header class="flex items-center gap-2.5 border-b border-line px-3 py-2">
    <Switch bind:checked={enabled} label={enabled ? "Disable rule" : "Enable rule"} small />
    <input
      bind:value={name}
      {placeholder}
      aria-label="Rule name"
      class="min-w-0 flex-1 bg-transparent text-[12.5px] font-medium outline-none placeholder:text-faint"
    />
    <button type="button" onclick={ondelete} title="Delete rule" class="grid size-6 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err">
      <Icon name="trash" size={13} />
    </button>
  </header>
  <div class="space-y-3 p-3">{@render children()}</div>
</section>
