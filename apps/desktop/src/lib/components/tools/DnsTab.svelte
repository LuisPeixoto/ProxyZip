<script lang="ts">
  import type { RuleSet } from "../../api";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  function add() {
    rules.dns.push({ name: "", enabled: true, host: "", ip: "127.0.0.1" });
  }
  const field = "mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
</script>

<TabFrame
  title="DNS overrides"
  description="Resolve a host name to an address you choose, without touching your hosts file. Applies to everything ProxyZip connects to, including tunnelled traffic."
  bind:enabled={rules.tools.dns}
  onadd={add}
  addLabel="Add override"
  count={rules.dns.length}
  empty="No DNS overrides yet."
>
  {#each rules.dns as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.dns.splice(i, 1)} placeholder="DNS override">
      <div class="flex items-center gap-2 text-[12.5px]">
        <input bind:value={rule.host} spellcheck="false" placeholder="app.example.com or *.example.test" aria-label="Host" class={field} />
        <span class="text-muted">→</span>
        <input bind:value={rule.ip} spellcheck="false" placeholder="127.0.0.1" aria-label="IP address" class={field} />
      </div>
    </RuleCard>
  {/each}
</TabFrame>
