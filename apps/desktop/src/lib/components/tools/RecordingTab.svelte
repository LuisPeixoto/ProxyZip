<script lang="ts">
  import { newMatcher, type RuleSet } from "../../api";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();
</script>

<TabFrame
  title="Record only"
  description="Keep the list focused: when this is on, only matching requests are recorded. Everything else is still proxied normally, it just doesn't show up."
  bind:enabled={rules.record_only.enabled}
>
  <div class="space-y-2">
    {#each rules.record_only.matchers as matcher, i (i)}
      <div class="flex items-start gap-2 rounded-xl border border-line bg-panel-2 p-3">
        <div class="flex-1"><MatcherEditor bind:matcher={rules.record_only.matchers[i]} placeholder="api.example.com" /></div>
        <button type="button" onclick={() => rules.record_only.matchers.splice(i, 1)} title="Remove" class="mt-1 grid size-6 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err"><Icon name="trash" size={13} /></button>
      </div>
    {:else}
      <p class="rounded-xl border border-dashed border-line px-4 py-6 text-center text-[12.5px] text-faint">Add at least one pattern, otherwise nothing is recorded while this is on.</p>
    {/each}
    <button type="button" onclick={() => rules.record_only.matchers.push(newMatcher())} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover"><Icon name="plus" size={14} /> Add pattern</button>
  </div>
</TabFrame>
