<script lang="ts">
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { app } from "../stores/app.svelte";
  import { api, type Preferences } from "../api";
  import { copy } from "../actions";
  import Icon from "./Icon.svelte";

  let prefs = $state<Preferences | null>(null);
  let include = $state("");
  let exclude = $state("");
  let saving = $state(false);
  let confirmRegen = $state(false);
  const st = $derived(app.status);

  $effect(() => {
    api.getSettings().then((p) => {
      prefs = p;
      include = p.ssl.include.join("\n");
      exclude = p.ssl.exclude.join("\n");
    });
  });

  const lines = (s: string) => s.split(/[\n,]/).map((l) => l.trim()).filter(Boolean);

  async function save() {
    if (!prefs) return;
    saving = true;
    const next: Preferences = { ...prefs, ssl: { ...prefs.ssl, include: lines(include), exclude: lines(exclude) } };
    const ok = await app.attempt(() => api.updateSettings(next), "Settings saved");
    saving = false;
    await app.refreshStatus();
    if (ok !== undefined) app.settingsOpen = false;
  }

  async function ca(action: "install" | "uninstall" | "regenerate") {
    if (action === "regenerate" && !confirmRegen) {
      confirmRegen = true;
      return;
    }
    confirmRegen = false;
    const fn = action === "install" ? api.installCa : action === "uninstall" ? api.uninstallCa : api.regenerateCa;
    await app.attempt(fn, action === "install" ? "Certificate installed" : action === "uninstall" ? "Certificate removed" : "New certificate generated — install it again");
    await app.refreshStatus();
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") app.settingsOpen = false;
  }
</script>

<svelte:window onkeydown={onkey} />

{#snippet section(title: string, icon: string)}
  <h3 class="mb-3 flex items-center gap-2 text-[12px] font-semibold tracking-wide text-muted uppercase"><Icon name={icon} size={14} />{title}</h3>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={() => (app.settingsOpen = false)}>
  <div class="shadow-pop flex max-h-full w-full max-w-[680px] flex-col overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" tabindex="-1">
    <div class="flex h-12 shrink-0 items-center justify-between border-b border-line px-5">
      <h2 class="text-[14px] font-semibold">Settings</h2>
      <button onclick={() => (app.settingsOpen = false)} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg"><Icon name="x" /></button>
    </div>
    {#if prefs}
      <div class="min-h-0 flex-1 space-y-7 overflow-y-auto p-5 text-[12.5px]">
        <section>
          {@render section("Certificate", "shield")}
          <div class="rounded-xl border border-line bg-panel-2 p-4">
            <div class="mb-3 flex items-center gap-2">
              <span class="size-2 rounded-full {st?.ca_installed ? 'bg-ok' : 'bg-warn'}"></span>
              <span class="font-medium">{st?.ca_installed ? "Trusted by this system" : "Not trusted yet"}</span>
            </div>
            <button onclick={() => st && revealItemInDir(st.ca_path)} class="selectable mono mb-3 block max-w-full truncate text-left text-[11px] text-muted hover:text-accent" title="Reveal in folder">{st?.ca_path}</button>
            <div class="flex flex-wrap gap-2">
              <button onclick={() => ca("install")} class="rounded-lg bg-accent px-3 py-1.5 font-medium text-white hover:opacity-90">Install & trust</button>
              <button onclick={() => ca("uninstall")} class="rounded-lg border border-line px-3 py-1.5 hover:bg-hover">Remove</button>
              <button onclick={() => ca("regenerate")} class="rounded-lg border px-3 py-1.5 {confirmRegen ? 'border-err text-err' : 'border-line hover:bg-hover'}">
                {confirmRegen ? "Click again to regenerate" : "Regenerate"}
              </button>
              {#if st?.platform === "macos"}
                <button onclick={() => app.attempt(() => api.installCaIosSimulator(), "Installed into iOS Simulator")} class="rounded-lg border border-line px-3 py-1.5 hover:bg-hover">iOS Simulator</button>
              {/if}
              <button onclick={async () => { const e = await app.attempt(() => api.terminalEnv()); if (e) copy(e, "Shell setup copied"); }} class="rounded-lg border border-line px-3 py-1.5 hover:bg-hover">Copy terminal setup</button>
            </div>
          </div>
        </section>

        <section>
          {@render section("Proxy", "globe")}
          <div class="grid grid-cols-[180px_1fr] items-center gap-x-4 gap-y-3">
            <label for="port" class="text-muted">Port</label>
            <input id="port" type="number" min="1" max="65535" bind:value={prefs.port} class="h-8 w-32 rounded-lg border border-line bg-panel-2 px-2.5 outline-none focus:border-accent" />
            <span class="text-muted">Allow LAN connections</span>
            <label class="flex items-center gap-2"><input type="checkbox" bind:checked={prefs.allow_lan} class="accent-[var(--accent)]" /> Listen on all interfaces (phones, emulators, VMs)</label>
          </div>
        </section>

        <section>
          {@render section("SSL proxying", "lock")}
          <label class="mb-3 flex items-center gap-2">
            <input type="checkbox" bind:checked={prefs.ssl.intercept_all} class="accent-[var(--accent)]" />
            Decrypt HTTPS for all hosts (otherwise only the include list)
          </label>
          <div class="grid grid-cols-2 gap-3">
            <label class="flex flex-col gap-1.5">
              <span class="text-muted">Include <span class="text-faint">(one per line, * wildcards)</span></span>
              <textarea bind:value={include} disabled={prefs.ssl.intercept_all} rows="5" placeholder="api.example.com&#10;*.myapp.io" class="selectable mono rounded-lg border border-line bg-panel-2 p-2 text-[12px] outline-none focus:border-accent disabled:opacity-40"></textarea>
            </label>
            <label class="flex flex-col gap-1.5">
              <span class="text-muted">Never decrypt <span class="text-faint">(pinned apps, banking…)</span></span>
              <textarea bind:value={exclude} rows="5" placeholder="*.apple.com&#10;*.icloud.com" class="selectable mono rounded-lg border border-line bg-panel-2 p-2 text-[12px] outline-none focus:border-accent"></textarea>
            </label>
          </div>
        </section>

        <section>
          {@render section("Capture", "layers")}
          <div class="grid grid-cols-[180px_1fr] items-center gap-x-4 gap-y-3">
            <label for="maxbody" class="text-muted">Max captured body</label>
            <div class="flex items-center gap-2"><input id="maxbody" type="number" min="1" bind:value={prefs.max_body_capture_mb} class="h-8 w-24 rounded-lg border border-line bg-panel-2 px-2.5 outline-none focus:border-accent" /> MB <span class="text-faint">(bodies are always forwarded in full)</span></div>
            <label for="maxflows" class="text-muted">Keep at most</label>
            <div class="flex items-center gap-2"><input id="maxflows" type="number" min="100" step="1000" bind:value={prefs.max_flows} class="h-8 w-28 rounded-lg border border-line bg-panel-2 px-2.5 outline-none focus:border-accent" /> flows <span class="text-faint">(applies after restart)</span></div>
            <span class="text-muted">Sessions</span>
            <label class="flex items-center gap-2"><input type="checkbox" bind:checked={prefs.restore_last_session} class="accent-[var(--accent)]" /> Restore the last session when ProxyZip starts</label>
            <span class="text-muted">Upstream certificates</span>
            <label class="flex items-center gap-2"><input type="checkbox" bind:checked={prefs.insecure_upstream} class="accent-[var(--accent)]" /> Don't verify server certificates <span class="text-warn">(insecure)</span></label>
          </div>
        </section>
      </div>
      <div class="flex h-14 shrink-0 items-center justify-end gap-2 border-t border-line px-5">
        <button onclick={() => (app.settingsOpen = false)} class="rounded-lg border border-line px-3.5 py-1.5 text-[12.5px] hover:bg-hover">Cancel</button>
        <button onclick={save} disabled={saving} class="rounded-lg bg-accent px-4 py-1.5 text-[12.5px] font-medium text-white hover:opacity-90 disabled:opacity-50">{saving ? "Saving…" : "Save"}</button>
      </div>
    {/if}
  </div>
</div>
