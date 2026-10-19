<script lang="ts">
  import { onMount } from "svelte";
  import { api, newMatcher, type RuleSet, type ThrottleProfile } from "../../api";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();
  let presets = $state<ThrottleProfile[]>([]);
  onMount(async () => (presets = await api.throttlePresets()));

  const number = "mono h-8 w-28 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent";
  const isCurrent = (p: ThrottleProfile) =>
    rules.throttle.profile.latency_ms === p.latency_ms && rules.throttle.profile.download_kbps === p.download_kbps && rules.throttle.profile.upload_kbps === p.upload_kbps;
</script>

<TabFrame
  title="Network conditions"
  description="Slow requests down to see how your app behaves on a bad connection. Latency is added before each request is sent, bandwidth limits apply to both directions."
  bind:enabled={rules.throttle.enabled}
>
  <div class="grid grid-cols-2 gap-2 md:grid-cols-4">
    {#each presets as p (p.name)}
      <button
        type="button"
        onclick={() => (rules.throttle.profile = { ...p })}
        class="rounded-xl border p-3 text-left transition {isCurrent(p) ? 'border-accent bg-accent-soft' : 'border-line bg-panel-2 hover:bg-hover'}"
      >
        <div class="flex items-center gap-1.5 text-[13px] font-semibold"><Icon name="gauge" size={14} /> {p.name}</div>
        <div class="mono mt-1 text-[11px] text-muted">{p.latency_ms} ms · ↓ {p.download_kbps >= 1000 ? `${p.download_kbps / 1000} Mbps` : `${p.download_kbps} kbps`}</div>
      </button>
    {/each}
  </div>

  <div class="rounded-xl border border-line bg-panel-2 p-3">
    <div class="mb-2 text-[12px] font-medium text-muted">Custom profile</div>
    <div class="flex flex-wrap items-end gap-3 text-[12.5px]">
      <label class="flex flex-col gap-1"><span class="text-muted">Latency (ms)</span><input type="number" min="0" bind:value={rules.throttle.profile.latency_ms} class={number} /></label>
      <label class="flex flex-col gap-1"><span class="text-muted">Download (kbps)</span><input type="number" min="0" bind:value={rules.throttle.profile.download_kbps} class={number} /></label>
      <label class="flex flex-col gap-1"><span class="text-muted">Upload (kbps)</span><input type="number" min="0" bind:value={rules.throttle.profile.upload_kbps} class={number} /></label>
      <span class="pb-1.5 text-[11px] text-faint">0 means unlimited</span>
    </div>
  </div>

  <div class="rounded-xl border border-line bg-panel-2 p-3">
    <div class="mb-2 flex items-center justify-between">
      <span class="text-[12px] font-medium text-muted">Only throttle matching requests</span>
      <button type="button" onclick={() => rules.throttle.matchers.push(newMatcher())} class="flex h-7 items-center gap-1 rounded-md px-2 text-[12px] text-accent hover:bg-accent-soft"><Icon name="plus" size={13} /> Add filter</button>
    </div>
    {#each rules.throttle.matchers as matcher, i (i)}
      <div class="mb-2 flex items-start gap-2">
        <div class="flex-1"><MatcherEditor bind:matcher={rules.throttle.matchers[i]} /></div>
        <button type="button" onclick={() => rules.throttle.matchers.splice(i, 1)} class="mt-1 grid size-6 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err"><Icon name="trash" size={13} /></button>
      </div>
    {:else}
      <p class="text-[12px] text-faint">Without filters every request is throttled.</p>
    {/each}
  </div>
</TabFrame>
