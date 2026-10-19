<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { newMatcher, type MapLocalRule, type RuleSet } from "../../api";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  function add() {
    rules.map_local.push({ name: "", enabled: true, matcher: newMatcher(), source: { type: "file", path: "" }, status: 200, headers: [], content_type: null });
  }

  async function pick(rule: MapLocalRule) {
    const directory = rule.source.type === "directory";
    const chosen = await open({ multiple: false, directory });
    if (typeof chosen === "string") rule.source.path = chosen;
  }
</script>

<TabFrame
  title="Map Local"
  description="Answer matching requests from a file or folder on your computer instead of the server. Perfect for mocking an API or testing a fix before it ships."
  bind:enabled={rules.tools.map_local}
  onadd={add}
  addLabel="Add Map Local rule"
  count={rules.map_local.length}
  empty="No local mappings yet."
>
  {#each rules.map_local as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.map_local.splice(i, 1)} placeholder="Map Local rule">
      <MatcherEditor bind:matcher={rule.matcher} placeholder="api.example.com/v1/me" />
      <div class="flex items-center gap-2 text-[12.5px]">
        <select
          value={rule.source.type}
          onchange={(e) => (rule.source = { type: e.currentTarget.value === "directory" ? "directory" : "file", path: "" })}
          class="h-8 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent"
        >
          <option value="file">Always use this file</option>
          <option value="directory">Look in this folder</option>
        </select>
        <input bind:value={rule.source.path} spellcheck="false" placeholder={rule.source.type === "file" ? "/path/to/response.json" : "/path/to/folder"} aria-label="Path" class="selectable mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent" />
        <button type="button" onclick={() => pick(rule)} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-2.5 hover:bg-hover"><Icon name="folder" size={14} /> Browse</button>
      </div>
      {#if rule.source.type === "directory"}
        <p class="text-[11px] text-faint">The part of the URL after the fixed part of the pattern picks the file, e.g. <span class="mono">example.com/static/*</span> serves <span class="mono">/static/app.js</span> from <span class="mono">&lt;folder&gt;/app.js</span>.</p>
      {/if}
      <div class="flex items-center gap-3 text-[12.5px]">
        <span class="text-muted">Status</span>
        <input type="number" min="100" max="599" bind:value={rule.status} class="mono h-8 w-20 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent" />
        <span class="text-muted">Content-Type</span>
        <input bind:value={rule.content_type} placeholder="guessed from the file" class="mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent" />
      </div>
    </RuleCard>
  {/each}
</TabFrame>
