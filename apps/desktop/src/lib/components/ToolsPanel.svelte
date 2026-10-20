<script lang="ts">
  import { app, type ToolsTab } from "../stores/app.svelte";
  import { api, errorText, type RuleSet } from "../api";
  import Icon from "./Icon.svelte";
  import BlockTab from "./tools/BlockTab.svelte";
  import BreakpointsTab from "./tools/BreakpointsTab.svelte";
  import DnsTab from "./tools/DnsTab.svelte";
  import MapLocalTab from "./tools/MapLocalTab.svelte";
  import MapRemoteTab from "./tools/MapRemoteTab.svelte";
  import NetworkTab from "./tools/NetworkTab.svelte";
  import ProtobufTab from "./tools/ProtobufTab.svelte";
  import RecordingTab from "./tools/RecordingTab.svelte";
  import ScriptsTab from "./tools/ScriptsTab.svelte";
  import MocksTab from "./tools/MocksTab.svelte";
  import SuitesTab from "./tools/SuitesTab.svelte";
  import RewriteTab from "./tools/RewriteTab.svelte";
  import ThrottleTab from "./tools/ThrottleTab.svelte";

  // Edit a copy and apply it on Save, so a half-typed rule never affects live traffic.
  let draft = $state<RuleSet>($state.snapshot(app.rules) as RuleSet);
  let saving = $state(false);

  const tabs: { id: ToolsTab; label: string; icon: string; count: (r: RuleSet) => number }[] = [
    { id: "block", label: "Block", icon: "ban", count: (r) => r.block.length },
    { id: "map_local", label: "Map Local", icon: "file", count: (r) => r.map_local.length },
    { id: "map_remote", label: "Map Remote", icon: "shuffle", count: (r) => r.map_remote.length },
    { id: "rewrite", label: "Rewrite", icon: "edit", count: (r) => r.rewrite.length },
    { id: "breakpoints", label: "Breakpoints", icon: "pause", count: (r) => r.breakpoints.length },
    { id: "scripts", label: "Scripts", icon: "code", count: (r) => r.scripts.length },
    { id: "mocks", label: "Mocks", icon: "server", count: (r) => r.mocks.length },
    { id: "suites", label: "Suites", icon: "play", count: () => 0 },
    { id: "protobuf", label: "Protobuf", icon: "layers", count: (r) => r.proto_mappings.length },
    { id: "throttle", label: "Throttling", icon: "gauge", count: (r) => (r.throttle.enabled ? 1 : 0) },
    { id: "dns", label: "DNS", icon: "globe", count: (r) => r.dns.length },
    { id: "network", label: "Proxy & ports", icon: "network", count: (r) => r.reverse.length + (r.upstream.enabled ? 1 : 0) + (r.socks.enabled ? 1 : 0) },
    { id: "recording", label: "Record only", icon: "eye", count: (r) => (r.record_only.enabled ? 1 : 0) },
  ];

  async function save() {
    saving = true;
    try {
      const rules = $state.snapshot(draft) as RuleSet;
      const errors = await api.saveRules(rules);
      app.rules = rules;
      for (const e of errors) app.toast(e, "err");
      await app.refreshStatus();
      if (!errors.length) app.toast("Rules applied", "ok");
      app.toolsOpen = false;
    } catch (e) {
      app.toast(errorText(e), "err");
    } finally {
      saving = false;
    }
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") app.toolsOpen = false;
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") save();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={() => (app.toolsOpen = false)}>
  <div class="shadow-pop flex h-full max-h-[760px] w-full max-w-[1000px] overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Tools" tabindex="-1">
    <nav class="flex w-[210px] shrink-0 flex-col gap-0.5 border-r border-line bg-panel-2 p-2.5">
      <div class="flex items-center gap-2 px-2.5 pt-1.5 pb-3 text-[14px] font-semibold"><Icon name="wrench" size={16} /> Tools</div>
      {#each tabs as t (t.id)}
        {@const n = t.count(draft)}
        <button
          onclick={() => (app.toolsTab = t.id)}
          class="flex h-8 items-center gap-2.5 rounded-lg px-2.5 text-[12.5px] {app.toolsTab === t.id ? 'bg-accent-soft font-medium text-accent' : 'text-fg/85 hover:bg-hover'}"
        >
          <Icon name={t.icon} size={15} />
          <span class="flex-1 text-left">{t.label}</span>
          {#if n}<span class="mono rounded-full bg-hover px-1.5 text-[10.5px] text-muted">{n}</span>{/if}
        </button>
      {/each}
    </nav>
    <div class="flex min-w-0 flex-1 flex-col">
      <div class="flex h-12 shrink-0 items-center justify-end border-b border-line px-4">
        <button onclick={() => (app.toolsOpen = false)} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Close"><Icon name="x" /></button>
      </div>
      <div class="selectable min-h-0 flex-1 overflow-y-auto p-5">
        {#if app.toolsTab === "block"}<BlockTab bind:rules={draft} />
        {:else if app.toolsTab === "map_local"}<MapLocalTab bind:rules={draft} />
        {:else if app.toolsTab === "map_remote"}<MapRemoteTab bind:rules={draft} />
        {:else if app.toolsTab === "rewrite"}<RewriteTab bind:rules={draft} />
        {:else if app.toolsTab === "breakpoints"}<BreakpointsTab bind:rules={draft} />
        {:else if app.toolsTab === "scripts"}<ScriptsTab bind:rules={draft} />
        {:else if app.toolsTab === "mocks"}<MocksTab bind:rules={draft} />
        {:else if app.toolsTab === "suites"}<SuitesTab />
        {:else if app.toolsTab === "protobuf"}<ProtobufTab bind:rules={draft} />
        {:else if app.toolsTab === "throttle"}<ThrottleTab bind:rules={draft} />
        {:else if app.toolsTab === "dns"}<DnsTab bind:rules={draft} />
        {:else if app.toolsTab === "network"}<NetworkTab bind:rules={draft} />
        {:else}<RecordingTab bind:rules={draft} />{/if}
      </div>
      <div class="flex h-14 shrink-0 items-center justify-between gap-2 border-t border-line px-5">
        <span class="text-[11.5px] text-faint">Rules apply to new requests as soon as you save.</span>
        <div class="flex gap-2">
          <button onclick={() => (app.toolsOpen = false)} class="rounded-lg border border-line px-3.5 py-1.5 text-[12.5px] hover:bg-hover">Cancel</button>
          <button onclick={save} disabled={saving} class="rounded-lg bg-accent px-4 py-1.5 text-[12.5px] font-medium text-white hover:opacity-90 disabled:opacity-50">{saving ? "Saving…" : "Save & apply"}</button>
        </div>
      </div>
    </div>
  </div>
</div>
