<script lang="ts">
  import { newMatcher, type RuleSet } from "../../api";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  function add() {
    rules.block.push({ name: "", enabled: true, matcher: newMatcher(), action: { type: "status", code: 403 } });
  }
</script>

<TabFrame
  title="Block"
  description="Stop matching requests from reaching the internet. Answer with an error status, or close the connection like a network failure."
  bind:enabled={rules.tools.block}
  onadd={add}
  addLabel="Add block rule"
  count={rules.block.length}
  empty="Nothing is blocked. Ads, trackers and flaky third parties are good candidates."
>
  {#each rules.block as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.block.splice(i, 1)} placeholder="Block rule">
      <MatcherEditor bind:matcher={rule.matcher} placeholder="ads.example.com" />
      <div class="flex items-center gap-3 text-[12.5px]">
        <span class="text-muted">Respond with</span>
        <select
          value={rule.action.type}
          onchange={(e) => (rule.action = e.currentTarget.value === "drop" ? { type: "drop" } : { type: "status", code: 403 })}
          class="h-8 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent"
        >
          <option value="status">an HTTP status</option>
          <option value="drop">a dropped connection</option>
        </select>
        {#if rule.action.type === "status"}
          <input type="number" min="100" max="599" bind:value={rule.action.code} aria-label="Status code" class="mono h-8 w-20 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent" />
        {/if}
      </div>
    </RuleCard>
  {/each}
</TabFrame>
