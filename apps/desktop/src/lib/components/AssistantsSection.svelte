<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api, type McpInfo } from "../api";
  import { copy } from "../actions";
  import Icon from "./Icon.svelte";

  let { onchange }: { onchange: (info: McpInfo) => void } = $props();

  type Client = "claude-code" | "cursor" | "vscode" | "claude-desktop";
  const clients: { id: Client; label: string }[] = [
    { id: "claude-code", label: "Claude Code" },
    { id: "cursor", label: "Cursor" },
    { id: "vscode", label: "VS Code" },
    { id: "claude-desktop", label: "Claude Desktop" },
  ];

  let info = $state<McpInfo | null>(null);
  let port = $state(9394);
  let client = $state<Client>("claude-code");
  let revealToken = $state(false);
  let busy = $state(false);

  $effect(() => {
    api.getMcpInfo().then((i) => {
      info = i;
      port = i.port;
    });
  });

  async function apply(enabled: boolean, allow: boolean) {
    busy = true;
    const next = await app.attempt(() => api.setMcp(enabled, port, allow));
    busy = false;
    if (next) {
      info = next;
      onchange(next);
    }
  }
  async function newToken() {
    const next = await app.attempt(() => api.regenerateMcpToken(), "New token generated: update your assistants");
    if (next) info = next;
  }

  const snippet = $derived.by(() => {
    if (!info) return "";
    const auth = `Bearer ${info.token}`;
    switch (client) {
      case "claude-code":
        return `claude mcp add --transport http proxyzip ${info.url} --header "Authorization: ${auth}"`;
      case "cursor":
        return JSON.stringify({ mcpServers: { proxyzip: { url: info.url, headers: { Authorization: auth } } } }, null, 2);
      case "vscode":
        return JSON.stringify({ servers: { proxyzip: { type: "http", url: info.url, headers: { Authorization: auth } } } }, null, 2);
      case "claude-desktop":
        return JSON.stringify({ mcpServers: { proxyzip: { command: "proxyzip", args: ["mcp"] } } }, null, 2);
    }
  });
  const shown = $derived(revealToken || !info ? snippet : snippet.replaceAll(info.token, "••••••••••••••••"));
</script>

{#if info}
  <div class="rounded-xl border border-line bg-panel-2 p-4">
    <label class="flex items-center gap-2">
      <input type="checkbox" checked={info.enabled} disabled={busy} onchange={(e) => apply(e.currentTarget.checked, info!.allow_changes)} class="accent-[var(--accent)]" />
      <span class="font-medium">Let AI assistants read my traffic</span>
      {#if info.running}<span class="rounded bg-ok/15 px-1.5 text-[10.5px] font-medium text-ok">on · {info.url}</span>{/if}
    </label>
    <p class="mt-1.5 text-[11.5px] leading-relaxed text-muted">
      Assistants such as Claude Code, Cursor and VS Code can list requests, read decoded bodies, run jq filters and generate code. Only this computer can connect, and every
      call needs the access token. Captured requests can contain passwords and tokens, so turn this on only for assistants you trust.
    </p>
    {#if info.error}<p class="mt-2 rounded-lg bg-err/8 px-3 py-2 text-[12px] text-err">{info.error}</p>{/if}

    {#if info.enabled}
      <div class="mt-3 flex flex-wrap items-center gap-x-5 gap-y-2">
        <label class="flex items-center gap-2 text-muted">Port
          <input type="number" min="1024" max="65535" bind:value={port} onchange={() => apply(true, info!.allow_changes)} class="h-7 w-24 rounded-lg border border-line bg-panel px-2 text-fg outline-none focus:border-accent" />
        </label>
        <label class="flex items-center gap-2">
          <input type="checkbox" checked={info.allow_changes} disabled={busy} onchange={(e) => apply(true, e.currentTarget.checked)} class="accent-[var(--accent)]" />
          Allow changes <span class="text-faint">(add rules, clear the list)</span>
        </label>
      </div>

      <div class="mt-4 flex flex-wrap gap-1">
        {#each clients as c (c.id)}
          <button onclick={() => (client = c.id)} class="rounded-md px-2.5 py-1 text-[11.5px] {client === c.id ? 'bg-accent-soft font-medium text-accent' : 'text-muted hover:bg-hover'}">{c.label}</button>
        {/each}
      </div>
      <pre class="selectable mono mt-2 max-h-40 overflow-auto rounded-lg border border-line bg-panel p-3 text-[11px] leading-snug whitespace-pre-wrap break-all text-fg/90">{shown}</pre>
      {#if client === "claude-desktop"}
        <p class="mt-1.5 text-[11.5px] text-muted">Needs the <span class="mono">proxyzip</span> command-line tool on your PATH. It finds this app by itself.</p>
      {/if}
      <div class="mt-2 flex flex-wrap gap-2">
        <button onclick={() => copy(snippet, "Copied. It includes your access token.")} class="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90"><Icon name="copy" size={13} /> Copy</button>
        <button onclick={() => (revealToken = !revealToken)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] hover:bg-hover">{revealToken ? "Hide token" : "Show token"}</button>
        <button onclick={newToken} class="rounded-lg border border-line px-3 py-1.5 text-[12px] hover:bg-hover">New token</button>
      </div>
    {/if}
  </div>
{/if}
