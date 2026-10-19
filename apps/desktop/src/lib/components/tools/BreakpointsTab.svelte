<script lang="ts">
  import { newMatcher, type RuleSet } from "../../api";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  function add() {
    rules.breakpoints.push({ name: "", enabled: true, matcher: newMatcher(), on_request: true, on_response: true });
  }
</script>

<TabFrame
  title="Breakpoints"
  description="Pause matching traffic, edit it, then let it continue. The editor opens whenever a request or response is held. Breakpoints only trigger while recording is on."
  bind:enabled={rules.tools.breakpoints}
  onadd={add}
  addLabel="Add breakpoint"
  count={rules.breakpoints.length}
  empty="No breakpoints yet."
>
  {#each rules.breakpoints as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.breakpoints.splice(i, 1)} placeholder="Breakpoint">
      <MatcherEditor bind:matcher={rule.matcher} />
      <div class="flex items-center gap-5 text-[12.5px]">
        <span class="text-muted">Pause the</span>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={rule.on_request} class="accent-[var(--accent)]" /> request</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={rule.on_response} class="accent-[var(--accent)]" /> response</label>
      </div>
    </RuleCard>
  {/each}
</TabFrame>
