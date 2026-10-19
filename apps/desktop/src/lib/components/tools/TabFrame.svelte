<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "../Icon.svelte";
  import Switch from "./Switch.svelte";

  let {
    title,
    description,
    enabled = $bindable<boolean | undefined>(undefined),
    onadd,
    addLabel = "Add rule",
    children,
    empty = "No rules yet.",
    count = 0,
  }: {
    title: string;
    description: string;
    enabled?: boolean;
    onadd?: () => void;
    addLabel?: string;
    children: Snippet;
    empty?: string;
    count?: number;
  } = $props();
</script>

<div class="space-y-4">
  <div class="flex items-start gap-3">
    <div class="min-w-0 flex-1">
      <h3 class="text-[15px] font-semibold">{title}</h3>
      <p class="mt-0.5 text-[12.5px] leading-relaxed text-muted">{description}</p>
    </div>
    {#if enabled !== undefined}
      <label class="flex shrink-0 items-center gap-2 text-[12px] text-muted">
        {enabled ? "On" : "Off"}
        <Switch bind:checked={enabled} label="Turn this tool on or off" />
      </label>
    {/if}
  </div>
  <div class="space-y-3 {enabled === false ? 'opacity-60' : ''}">
    {@render children()}
    {#if onadd}
      {#if count === 0}
        <p class="rounded-xl border border-dashed border-line px-4 py-6 text-center text-[12.5px] text-faint">{empty}</p>
      {/if}
      <button type="button" onclick={onadd} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover">
        <Icon name="plus" size={14} /> {addLabel}
      </button>
    {/if}
  </div>
</div>
