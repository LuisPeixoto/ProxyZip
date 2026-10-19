<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/stores/app.svelte";
  import { api } from "./lib/api";
  import { compare, openSession, remove, repeat, saveSession } from "./lib/actions";
  import CodeModal from "./lib/components/CodeModal.svelte";
  import DiffModal from "./lib/components/DiffModal.svelte";
  import OpenSessionDialog from "./lib/components/OpenSessionDialog.svelte";
  import BreakpointEditor from "./lib/components/BreakpointEditor.svelte";
  import Composer from "./lib/components/Composer.svelte";
  import ToolsPanel from "./lib/components/ToolsPanel.svelte";
  import Toolbar from "./lib/components/Toolbar.svelte";
  import Sidebar from "./lib/components/Sidebar.svelte";
  import FilterBar from "./lib/components/FilterBar.svelte";
  import FlowTable from "./lib/components/FlowTable.svelte";
  import FlowDetail from "./lib/components/FlowDetail.svelte";
  import SettingsModal from "./lib/components/SettingsModal.svelte";
  import Toasts from "./lib/components/Toasts.svelte";

  let searchEl: HTMLInputElement | undefined = $state();
  let main: HTMLDivElement | undefined = $state();

  onMount(() => {
    app.init();
  });

  $effect(() => {
    document.documentElement.dataset.theme = app.theme;
  });

  function onkeydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (mod && e.key.toLowerCase() === "k" && e.shiftKey) {
      e.preventDefault();
      api.clearFlows();
    } else if (mod && e.key.toLowerCase() === "k") {
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    } else if (mod && e.key.toLowerCase() === "e") {
      e.preventDefault();
      if (app.status) api.setRecording(!app.status.recording).then(() => app.refreshStatus());
    } else if (mod && e.key.toLowerCase() === "r") {
      e.preventDefault(); // never reload the webview
      if (app.focused != null) repeat(app.focused);
    } else if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      saveSession(app.selected.size > 1 ? [...app.selected] : null);
    } else if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      openSession();
    } else if (mod && e.key.toLowerCase() === "d") {
      e.preventDefault();
      compare([...app.selected]);
    } else if (mod && e.key.toLowerCase() === "t") {
      e.preventDefault();
      app.openTools();
    } else if (mod && e.key.toLowerCase() === "n") {
      e.preventDefault();
      app.openComposer();
    } else if (mod && e.key === ",") {
      e.preventDefault();
      app.settingsOpen = true;
    } else if (e.key === "Escape" && typing) {
      (e.target as HTMLElement).blur();
    } else if (!typing && mod && e.key === "Backspace") {
      e.preventDefault();
      remove(app.selectedIds());
    }
  }

  function startResize(e: PointerEvent) {
    const rect = main!.getBoundingClientRect();
    const move = (ev: PointerEvent) => app.setDetailSize(Math.min(rect.height - 140, Math.max(140, rect.bottom - ev.clientY)));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

<svelte:window {onkeydown} oncontextmenu={(e) => e.preventDefault()} />

<div class="flex h-full flex-col">
  <Toolbar bind:searchEl />
  <div class="flex min-h-0 flex-1">
    {#if app.sidebarOpen}<Sidebar />{/if}
    <div bind:this={main} class="flex min-w-0 flex-1 flex-col">
      <FilterBar />
      <FlowTable />
      {#if app.flows.length}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div onpointerdown={startResize} class="h-[5px] shrink-0 cursor-row-resize border-y border-line bg-panel-2 hover:bg-accent/40"></div>
        <div class="min-h-0 shrink-0" style="height:{app.detailSize}px">
          <FlowDetail />
        </div>
      {/if}
    </div>
  </div>
</div>

{#if app.settingsOpen}<SettingsModal />{/if}
{#if app.toolsOpen}<ToolsPanel />{/if}
{#if app.composer}<Composer />{/if}
{#if app.codeFor != null}<CodeModal />{/if}
{#if app.diff}<DiffModal />{/if}
{#if app.pendingOpen}<OpenSessionDialog />{/if}
<BreakpointEditor />
<Toasts />
