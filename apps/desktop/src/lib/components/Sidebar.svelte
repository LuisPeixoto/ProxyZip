<script lang="ts">
  import { app, type SidebarSel } from "../stores/app.svelte";
  import Icon from "./Icon.svelte";

  let hostFilter = $state("");
  let hostsOpen = $state(true);
  let clientsOpen = $state(true);

  const hosts = $derived(
    hostFilter ? app.hosts.filter(([h]) => h.toLowerCase().includes(hostFilter.toLowerCase())) : app.hosts,
  );

  function isSel(s: SidebarSel): boolean {
    const cur = app.sidebar;
    if (cur.kind !== s.kind) return false;
    if ("value" in s && "value" in cur) return s.value === cur.value;
    return true;
  }

  function hue(host: string): number {
    let h = 0;
    for (const c of host) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  }

  function avatar(host: string): string {
    const parts = host.split(".");
    const main = parts.length >= 2 ? parts[parts.length - 2] : host;
    return (main[0] ?? "?").toUpperCase();
  }
</script>

<aside class="flex w-[236px] shrink-0 flex-col border-r border-line bg-panel-2">
  <nav class="space-y-0.5 p-2 pt-3">
    {#each [
      { sel: { kind: "all" } as SidebarSel, icon: "layers", label: "All traffic", count: app.counts.all },
      { sel: { kind: "pinned" } as SidebarSel, icon: "pin", label: "Pinned", count: app.counts.pinned },
      { sel: { kind: "errors" } as SidebarSel, icon: "alert", label: "Errors", count: app.counts.errors },
      { sel: { kind: "ws" } as SidebarSel, icon: "message", label: "WebSockets", count: app.counts.ws },
    ] as item (item.label)}
      <button
        onclick={() => (app.sidebar = item.sel)}
        class="flex h-8 w-full items-center gap-2.5 rounded-lg px-2.5 text-[12.5px] {isSel(item.sel)
          ? 'bg-accent-soft font-medium text-accent'
          : 'text-fg/85 hover:bg-hover'}"
      >
        <Icon name={item.icon} size={15} />
        <span class="flex-1 text-left">{item.label}</span>
        {#if item.count}
          <span class="mono text-[10.5px] {isSel(item.sel) ? 'text-accent' : 'text-faint'}">{item.count.toLocaleString()}</span>
        {/if}
      </button>
    {/each}
  </nav>

  <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
    {#if app.clients.length > 1 || (app.clients.length === 1 && app.clients[0][0] !== "This computer")}
      <button onclick={() => (clientsOpen = !clientsOpen)} class="mt-2 flex w-full items-center gap-1 px-2 py-1.5 text-[10.5px] font-semibold tracking-wider text-faint uppercase">
        <Icon name={clientsOpen ? "chevronDown" : "chevronRight"} size={12} /> Clients
      </button>
      {#if clientsOpen}
        {#each app.clients as [name, count] (name)}
          {@const sel = { kind: "client", value: name } as SidebarSel}
          <button
            onclick={() => (app.sidebar = sel)}
            class="flex h-7 w-full items-center gap-2 rounded-md px-2.5 text-[12px] {isSel(sel) ? 'bg-accent-soft text-accent' : 'text-fg/85 hover:bg-hover'}"
          >
            <Icon name={name === "This computer" ? "laptop" : "smartphone"} size={14} class="text-muted" />
            <span class="flex-1 truncate text-left">{name}</span>
            <span class="mono text-[10.5px] text-faint">{count}</span>
          </button>
        {/each}
      {/if}
    {/if}

    <div class="mt-2 flex items-center pr-1">
      <button onclick={() => (hostsOpen = !hostsOpen)} class="flex flex-1 items-center gap-1 px-2 py-1.5 text-[10.5px] font-semibold tracking-wider text-faint uppercase">
        <Icon name={hostsOpen ? "chevronDown" : "chevronRight"} size={12} /> Domains
        <span class="ml-1 font-normal normal-case">{app.hosts.length || ""}</span>
      </button>
    </div>
    {#if hostsOpen}
      {#if app.hosts.length > 8}
        <input
          bind:value={hostFilter}
          placeholder="Find domain…"
          class="mb-1 h-7 w-full rounded-md border border-line bg-panel px-2 text-[12px] outline-none placeholder:text-faint focus:border-accent"
        />
      {/if}
      {#each hosts as [host, info] (host)}
        {@const sel = { kind: "host", value: host } as SidebarSel}
        <button
          onclick={() => (app.sidebar = sel)}
          title={host}
          class="group flex h-7 w-full items-center gap-2 rounded-md px-2 text-[12px] {isSel(sel) ? 'bg-accent-soft text-accent' : 'text-fg/85 hover:bg-hover'}"
        >
          <span
            class="grid size-[18px] shrink-0 place-items-center rounded-[5px] text-[10px] font-bold text-white"
            style="background: linear-gradient(135deg, hsl({hue(host)} 70% 58%), hsl({(hue(host) + 40) % 360} 70% 48%))"
          >{avatar(host)}</span>
          <span class="flex-1 truncate text-left">{host}</span>
          {#if info.secure}<Icon name="lock" size={11} class="text-faint" />{/if}
          {#if info.errors}<span class="size-1.5 rounded-full bg-err"></span>{/if}
          <span class="mono text-[10.5px] text-faint">{info.count}</span>
        </button>
      {:else}
        <p class="px-2.5 py-2 text-[12px] text-faint">Domains appear here as traffic arrives.</p>
      {/each}
    {/if}
  </div>
</aside>
