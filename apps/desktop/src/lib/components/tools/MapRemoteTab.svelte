<script lang="ts">
  import { newMatcher, type RuleSet } from "../../api";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  function add() {
    rules.map_remote.push({ name: "", enabled: true, matcher: newMatcher(), scheme: null, host: "", port: null, path: "", keep_host_header: false });
  }
  const input = "mono h-8 min-w-0 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
</script>

<TabFrame
  title="Map Remote"
  description="Send matching requests to a different server: point production at staging, or an API at your laptop. Leave a field empty to keep the original value."
  bind:enabled={rules.tools.map_remote}
  onadd={add}
  addLabel="Add Map Remote rule"
  count={rules.map_remote.length}
  empty="No remote mappings yet."
>
  {#each rules.map_remote as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.map_remote.splice(i, 1)} placeholder="Map Remote rule">
      <MatcherEditor bind:matcher={rule.matcher} placeholder="api.example.com/*" />
      <div class="grid grid-cols-[88px_1fr_88px] gap-2 text-[12.5px]">
        <label class="flex flex-col gap-1"><span class="text-muted">Scheme</span>
          <select bind:value={rule.scheme} class="h-8 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent">
            <option value={null}>Keep</option><option value="http">http</option><option value="https">https</option>
          </select>
        </label>
        <label class="flex flex-col gap-1"><span class="text-muted">Host</span><input bind:value={rule.host} spellcheck="false" placeholder="staging.example.com" class={input} /></label>
        <label class="flex flex-col gap-1"><span class="text-muted">Port</span><input type="number" min="1" max="65535" bind:value={rule.port} placeholder="Keep" class={input} /></label>
      </div>
      <label class="flex flex-col gap-1 text-[12.5px]"><span class="text-muted">Path <span class="text-faint">(replaces the whole path; the query string is kept)</span></span>
        <input bind:value={rule.path} spellcheck="false" placeholder="/v2/items" class={input} />
      </label>
      <label class="flex items-center gap-2 text-[12.5px]"><input type="checkbox" bind:checked={rule.keep_host_header} class="accent-[var(--accent)]" /> Keep the original <span class="mono">Host</span> header</label>
    </RuleCard>
  {/each}
</TabFrame>
