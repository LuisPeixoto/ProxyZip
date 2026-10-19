<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api } from "../api";
  import Icon from "./Icon.svelte";

  let { searchEl = $bindable() }: { searchEl?: HTMLInputElement } = $props();
  const st = $derived(app.status);
  const isMac = $derived(st?.platform === "macos");

  async function toggleRecording() {
    if (!st) return;
    await app.attempt(() => api.setRecording(!st.recording));
    await app.refreshStatus();
  }

  async function toggleSystemProxy() {
    if (!st) return;
    await app.attempt(
      () => api.setSystemProxy(!st.system_proxy),
      st.system_proxy ? "System proxy disabled" : "System proxy enabled — all apps now go through ProxyZip",
    );
    await app.refreshStatus();
  }

  async function clear() {
    await app.attempt(() => api.clearFlows());
  }
</script>

<header
  data-tauri-drag-region
  class="flex h-[52px] shrink-0 items-center gap-2 border-b border-line bg-panel px-3"
  class:pl-[84px]={isMac}
>
  <button
    title={st?.recording ? "Pause recording (⌘E)" : "Resume recording (⌘E)"}
    onclick={toggleRecording}
    class="group flex h-8 items-center gap-2 rounded-lg px-2.5 text-[12px] font-medium hover:bg-hover"
  >
    {#if st?.recording}
      <span class="recording-dot size-2.5 rounded-full bg-err"></span>
      <span class="text-fg">Recording</span>
    {:else}
      <span class="size-2.5 rounded-full border-2 border-faint"></span>
      <span class="text-muted">Paused</span>
    {/if}
  </button>

  <button title="Clear session (⌘⇧K)" onclick={clear} class="grid size-8 place-items-center rounded-lg text-muted hover:bg-hover hover:text-fg">
    <Icon name="trash" />
  </button>

  <div class="mx-1 h-5 w-px bg-line"></div>

  <div
    class="flex h-8 items-center gap-2 rounded-lg border border-line bg-panel-2 px-2.5 text-[12px]"
    title={st?.error ?? "Proxy address"}
  >
    <span class="size-1.5 rounded-full {st?.running ? 'bg-ok' : 'bg-err'}"></span>
    <span class="mono text-muted">{st?.running ? st.listen : st?.error ? "Proxy stopped" : "Starting…"}</span>
  </div>

  <button
    onclick={toggleSystemProxy}
    disabled={!st?.running}
    title="Route the whole system through ProxyZip"
    class="flex h-8 items-center gap-2 rounded-lg px-2.5 text-[12px] font-medium transition {st?.system_proxy
      ? 'bg-accent text-white shadow-[0_0_0_1px_var(--accent),0_6px_20px_-6px_var(--accent)]'
      : 'border border-line text-muted hover:bg-hover hover:text-fg'}"
  >
    <Icon name="monitor" size={14} />
    System proxy {st?.system_proxy ? "on" : "off"}
  </button>

  <button
    onclick={() => (app.settingsOpen = true)}
    title={st?.ca_detail}
    class="flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium {st?.ca_installed
      ? 'text-ok hover:bg-hover'
      : 'bg-warn/12 text-warn hover:bg-warn/20'}"
  >
    <Icon name={st?.ca_installed ? "shieldCheck" : "shield"} size={14} />
    {st?.ca_installed ? "HTTPS ready" : "Install certificate"}
  </button>

  <button
    onclick={() => app.openTools()}
    title="Tools: block, map, rewrite, breakpoints, network conditions… (⌘T)"
    class="flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium text-muted hover:bg-hover hover:text-fg"
  >
    <Icon name="wrench" size={14} /> <span class="hidden xl:inline">Tools</span>
  </button>
  <button
    onclick={() => app.openComposer()}
    title="Compose a request (⌘N)"
    class="flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium text-muted hover:bg-hover hover:text-fg"
  >
    <Icon name="send" size={14} /> <span class="hidden xl:inline">Compose</span>
  </button>
  {#if app.paused.length}
    <button
      onclick={() => (app.breakpointHidden = false)}
      title="Requests are waiting at a breakpoint"
      class="flex h-8 items-center gap-1.5 rounded-lg bg-warn/15 px-2.5 text-[12px] font-medium text-warn hover:bg-warn/25"
    >
      <Icon name="pause" size={14} /> {app.paused.length} paused
    </button>
  {/if}
  {#if app.throttleLabel}
    <button
      onclick={() => app.openTools("throttle")}
      title="Network throttling is on"
      class="flex h-8 items-center gap-1.5 rounded-lg bg-accent-soft px-2.5 text-[12px] font-medium text-accent hover:opacity-80"
    >
      <Icon name="gauge" size={14} /> {app.throttleLabel}
    </button>
  {/if}

  <div data-tauri-drag-region class="flex flex-1 justify-center px-2">
    <label class="group relative flex h-8 w-full max-w-[560px] items-center">
      <Icon name="search" size={14} class="pointer-events-none absolute left-2.5 text-faint" />
      <input
        bind:this={searchEl}
        bind:value={app.search}
        spellcheck="false"
        placeholder="Filter  —  text, method:post, status:4xx, host:api, type:json, -exclude, /regex/"
        class="h-full w-full rounded-lg border border-line bg-panel-2 pr-14 pl-8 text-[12.5px] outline-none placeholder:text-faint focus:border-accent focus:bg-panel focus:ring-3 focus:ring-accent-soft"
      />
      {#if app.search}
        <button onclick={() => (app.search = "")} class="absolute right-2 grid size-5 place-items-center rounded text-faint hover:text-fg">
          <Icon name="x" size={12} />
        </button>
      {:else}
        <kbd class="absolute right-2 rounded border border-line px-1.5 py-px text-[10px] text-faint">⌘K</kbd>
      {/if}
    </label>
  </div>

  <span class="mono hidden text-[11px] text-faint lg:inline">
    {app.filtered.length.toLocaleString()} / {app.flows.length.toLocaleString()}
  </span>

  <button
    title="Toggle sidebar"
    onclick={() => (app.sidebarOpen = !app.sidebarOpen)}
    class="grid size-8 place-items-center rounded-lg text-muted hover:bg-hover hover:text-fg"
  >
    <Icon name="sidebar" />
  </button>
  <button
    title="Toggle theme"
    onclick={() => app.setTheme(app.theme === "dark" ? "light" : "dark")}
    class="grid size-8 place-items-center rounded-lg text-muted hover:bg-hover hover:text-fg"
  >
    <Icon name={app.theme === "dark" ? "sun" : "moon"} />
  </button>
  <button
    title="Settings (⌘,)"
    onclick={() => (app.settingsOpen = true)}
    class="grid size-8 place-items-center rounded-lg text-muted hover:bg-hover hover:text-fg"
  >
    <Icon name="settings" />
  </button>
</header>
