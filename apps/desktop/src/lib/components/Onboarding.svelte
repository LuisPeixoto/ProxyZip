<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api } from "../api";
  import { copy, requestOpen } from "../actions";
  import Icon from "./Icon.svelte";

  const st = $derived(app.status);
  const addr = $derived(st?.host && st.port ? `${st.host}:${st.port}` : "—");

  async function installCa() {
    await app.attempt(() => api.installCa(), "Certificate installed — HTTPS traffic can now be decrypted");
    await app.refreshStatus();
  }
  async function systemProxy() {
    await app.attempt(() => api.setSystemProxy(true), "System proxy enabled");
    await app.refreshStatus();
  }
  async function terminal() {
    const env = await app.attempt(() => api.terminalEnv());
    if (env) await copy(env, "Paste into a terminal to route it through ProxyZip");
  }
</script>

<div class="min-h-0 flex-1 overflow-auto">
  <div class="mx-auto flex max-w-[860px] flex-col items-center px-8 py-12">
    <div class="relative mb-5">
      <div class="absolute inset-0 rounded-[26px] bg-accent opacity-40 blur-2xl"></div>
      <div class="relative grid size-[72px] place-items-center rounded-[22px] bg-gradient-to-br from-[#7c5cff] via-[#5b6cff] to-[#22d3ee] text-white shadow-xl">
        <Icon name="zap" size={36} stroke={2.2} />
      </div>
    </div>
    <h1 class="text-[26px] font-semibold tracking-tight">Waiting for traffic</h1>
    <p class="mt-1.5 text-center text-[13.5px] text-muted">
      ProxyZip is listening on <button onclick={() => copy(addr, "Proxy address copied")} class="mono rounded bg-hover px-1.5 py-0.5 text-fg hover:bg-accent-soft">{addr}</button>.
      Point an app, browser, terminal or device at it.
    </p>

    <div class="mt-9 grid w-full grid-cols-1 gap-3 md:grid-cols-3">
      <div class="rounded-2xl border border-line bg-panel-2 p-5">
        <div class="mb-3 flex items-center gap-2">
          <span class="grid size-7 place-items-center rounded-lg {st?.ca_installed ? 'bg-ok/15 text-ok' : 'bg-accent-soft text-accent'}">
            <Icon name={st?.ca_installed ? "check" : "shield"} size={15} />
          </span>
          <span class="text-[13px] font-semibold">1 · Trust the certificate</span>
        </div>
        <p class="mb-4 text-[12px] leading-relaxed text-muted">Lets ProxyZip decrypt HTTPS. The key never leaves this computer.</p>
        {#if st?.ca_installed}
          <span class="text-[12px] font-medium text-ok">Installed and trusted</span>
        {:else}
          <button onclick={installCa} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Install certificate</button>
        {/if}
      </div>

      <div class="rounded-2xl border border-line bg-panel-2 p-5">
        <div class="mb-3 flex items-center gap-2">
          <span class="grid size-7 place-items-center rounded-lg {st?.system_proxy ? 'bg-ok/15 text-ok' : 'bg-accent-soft text-accent'}">
            <Icon name={st?.system_proxy ? "check" : "monitor"} size={15} />
          </span>
          <span class="text-[13px] font-semibold">2 · Capture this computer</span>
        </div>
        <p class="mb-4 text-[12px] leading-relaxed text-muted">Route browsers and apps that honour the system proxy. Restored when you quit.</p>
        {#if st?.system_proxy}
          <span class="text-[12px] font-medium text-ok">System proxy is on</span>
        {:else}
          <button onclick={systemProxy} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Enable system proxy</button>
        {/if}
      </div>

      <div class="rounded-2xl border border-line bg-panel-2 p-5">
        <div class="mb-3 flex items-center gap-2">
          <span class="grid size-7 place-items-center rounded-lg bg-accent-soft text-accent"><Icon name="terminal" size={15} /></span>
          <span class="text-[13px] font-semibold">3 · Or just a terminal</span>
        </div>
        <p class="mb-4 text-[12px] leading-relaxed text-muted">Exports proxy variables and CA paths for curl, Node, Python, Ruby, Go, Deno…</p>
        <button onclick={terminal} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium hover:bg-hover">Copy shell setup</button>
      </div>
    </div>

    {#if app.recent.length}
      <div class="mt-3 w-full rounded-2xl border border-line bg-panel-2 p-5">
        <div class="mb-3 flex items-center gap-2 text-[13px] font-semibold"><Icon name="history" size={15} class="text-accent" /> Recent sessions</div>
        <div class="grid gap-1">
          {#each app.recent.slice(0, 5) as session (session.path)}
            <button onclick={() => requestOpen(session.path)} class="flex items-center gap-3 rounded-lg px-2.5 py-1.5 text-left text-[12.5px] hover:bg-hover" title={session.path}>
              <Icon name="file" size={14} class="shrink-0 text-muted" />
              <span class="min-w-0 flex-1 truncate font-medium">{session.name}</span>
              <span class="mono shrink-0 text-[11px] text-faint">{session.flows.toLocaleString()} requests</span>
              <span class="shrink-0 text-[11px] text-faint">{new Date(session.saved_at).toLocaleDateString()}</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <button onclick={() => (app.setupOpen = "ios")} class="mt-3 flex w-full items-start gap-3 rounded-2xl border border-line bg-panel-2 p-5 text-left hover:bg-hover">
      <span class="grid size-7 shrink-0 place-items-center rounded-lg bg-accent-soft text-accent"><Icon name="smartphone" size={15} /></span>
      <div class="text-[12px] leading-relaxed text-muted">
        <div class="mb-1 text-[13px] font-semibold text-fg">Phones, emulators, browsers & runtimes</div>
        Guided setup for iPhone and iPad, Android, Firefox, Java, Node.js, Python and Ruby. Open <b class="text-fg">Devices & apps</b>.
      </div>
    </button>
  </div>
</div>
