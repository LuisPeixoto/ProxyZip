<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { app } from "../stores/app.svelte";
  import { api, type PluginList } from "../api";
  import Icon from "./Icon.svelte";

  let list = $state<PluginList | null>(null);
  let busy = $state(false);

  $effect(() => {
    api.listPlugins().then((l) => (list = l));
  });

  async function run(fn: () => Promise<PluginList>, message?: string) {
    busy = true;
    const next = await app.attempt(fn, message);
    busy = false;
    if (next) list = next;
  }

  async function install(kind: "folder" | "zip") {
    const picked = await open(kind === "folder" ? { directory: true, title: "Choose a plugin folder" } : { filters: [{ name: "Plugin", extensions: ["zip"] }], title: "Choose a plugin .zip" });
    if (typeof picked === "string") await run(() => api.installPlugin(picked), "Plugin installed");
  }

  function parts(p: PluginList["plugins"][number]): string {
    const c = p.contributes;
    const out: string[] = [];
    if (c.hooks) out.push([c.hooks.request && "request", c.hooks.response && "response"].filter(Boolean).join(" + ") + " hook");
    if (c.viewers.length) out.push(`${c.viewers.length} viewer${c.viewers.length === 1 ? "" : "s"}`);
    if (c.copy_as.length) out.push(`${c.copy_as.length} copy-as`);
    return out.join(" · ");
  }
</script>

<div class="rounded-xl border border-line bg-panel-2 p-4">
  <p class="text-[11.5px] leading-relaxed text-muted">
    Plugins are small JavaScript packages that add traffic hooks, body viewers and “copy as” formats. They run in the same sandbox as scripts, with no file or network access, but a hook plugin can read and change the traffic it matches. Install only plugins you trust.
  </p>
  {#if list}
    <div class="mt-3 grid gap-2">
      {#each list.plugins as p (p.id)}
        <div class="flex items-start gap-3 rounded-lg border border-line bg-panel px-3 py-2.5">
          <input type="checkbox" checked={p.enabled} disabled={busy} onchange={(e) => run(() => api.setPluginEnabled(p.id, e.currentTarget.checked))} class="mt-1 accent-[var(--accent)]" aria-label="Enable {p.name}" />
          <div class="min-w-0 flex-1">
            <div class="flex items-baseline gap-2"><span class="font-medium">{p.name}</span><span class="mono text-[11px] text-faint">{p.version}</span>{#if p.author}<span class="text-[11px] text-faint">by {p.author}</span>{/if}</div>
            {#if p.description}<div class="text-[11.5px] text-muted">{p.description}</div>{/if}
            <div class="mt-0.5 text-[11px] text-faint">{parts(p)}</div>
          </div>
          <button disabled={busy} onclick={() => run(() => api.removePlugin(p.id), "Plugin removed")} class="rounded-md px-2 py-1 text-[11.5px] text-muted hover:bg-hover hover:text-err">Remove</button>
        </div>
      {:else}
        <div class="rounded-lg border border-dashed border-line px-3 py-4 text-center text-[12px] text-faint">No plugins installed.</div>
      {/each}
      {#each list.errors as e (e.folder)}
        <div class="rounded-lg bg-err/8 px-3 py-2 text-[12px] text-err"><b>{e.folder}</b>: {e.message}</div>
      {/each}
    </div>
    <div class="mt-3 flex flex-wrap gap-2">
      <button disabled={busy} onclick={() => install("folder")} class="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-50"><Icon name="plus" size={13} /> Install from folder…</button>
      <button disabled={busy} onclick={() => install("zip")} class="rounded-lg border border-line px-3 py-1.5 text-[12px] hover:bg-hover disabled:opacity-50">Install from .zip…</button>
      <button onclick={() => list && revealItemInDir(list.dir)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] hover:bg-hover">Show plugins folder</button>
    </div>
  {/if}
</div>
