<script lang="ts">
  import { app, type SetupTab } from "../../stores/app.svelte";
  import { api, type AndroidDevice, type FirefoxReport, type IosInfo, type JavaInfo } from "../../api";
  import { copy } from "../../actions";
  import Icon from "../Icon.svelte";

  const tabs: { id: SetupTab; label: string; icon: string }[] = [
    { id: "computer", label: "This computer", icon: "monitor" },
    { id: "terminal", label: "Terminal & runtimes", icon: "terminal" },
    { id: "ios", label: "iPhone & iPad", icon: "smartphone" },
    { id: "android", label: "Android", icon: "smartphone" },
    { id: "java", label: "Java", icon: "code" },
    { id: "firefox", label: "Firefox", icon: "globe" },
  ];
  const st = $derived(app.status);
  const tab = $derived(app.setupOpen ?? "computer");
  const proxyAddr = $derived(st?.host && st.port ? `${st.host}:${st.port}` : "—");
  /** Addresses other than this computer that have sent traffic. */
  const remoteClients = $derived(app.clients.filter(([name]) => !name.startsWith("127.") && name !== "::1" && name !== "localhost"));

  let shell = $state("auto");
  let envText = $state("");
  let ios = $state<IosInfo | null>(null);
  let devices = $state<AndroidDevice[]>([]);
  let androidError = $state<string | null>(null);
  let androidNote = $state<string | null>(null);
  let java = $state<JavaInfo | null>(null);
  let firefox = $state<FirefoxReport | null>(null);
  let busy = $state(false);

  $effect(() => {
    if (tab === "terminal") loadEnv();
    if (tab === "ios" && !ios) api.iosSetup().then((v) => (ios = v)).catch(() => {});
    if (tab === "android") refreshDevices();
    if (tab === "java" && !java) api.javaSetup().then((v) => (java = v)).catch(() => {});
  });

  async function loadEnv() {
    envText = (await app.attempt(() => api.terminalEnv(shell === "auto" ? undefined : shell))) ?? "";
  }
  async function refreshDevices() {
    androidError = null;
    try {
      devices = await api.androidDevices();
    } catch (e) {
      devices = [];
      androidError = String(e);
    }
  }
  async function androidProxy(d: AndroidDevice, enable: boolean) {
    busy = true;
    await app.attempt(() => api.androidSetProxy(d.serial, enable), enable ? `${d.serial} now uses ProxyZip` : `${d.serial} no longer uses a proxy`);
    busy = false;
  }
  async function androidCert(d: AndroidDevice, system: boolean) {
    busy = true;
    androidNote = (await app.attempt(() => api.androidInstallCert(d.serial, system))) ?? null;
    busy = false;
  }
  async function installFirefox() {
    busy = true;
    firefox = (await app.attempt(() => api.installFirefox())) ?? null;
    busy = false;
  }
  async function installCa() {
    await app.attempt(() => api.installCa(), "Certificate installed");
    await app.refreshStatus();
  }
  async function systemProxy(on: boolean) {
    await app.attempt(() => api.setSystemProxy(on), on ? "System proxy enabled" : "System proxy disabled");
    await app.refreshStatus();
  }
  function close() {
    app.setupOpen = null;
  }
  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={close}>
  <div class="shadow-pop flex h-full max-h-[720px] w-full max-w-[980px] overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Devices and apps" tabindex="-1">
    <nav class="flex w-[220px] shrink-0 flex-col gap-0.5 border-r border-line bg-panel-2 p-2.5">
      <div class="flex items-center gap-2 px-2.5 pt-1.5 pb-3 text-[14px] font-semibold"><Icon name="smartphone" size={16} /> Devices & apps</div>
      {#each tabs as t (t.id)}
        <button onclick={() => (app.setupOpen = t.id)} class="flex h-9 items-center gap-2.5 rounded-lg px-2.5 text-left text-[12.5px] {tab === t.id ? 'bg-accent-soft font-medium text-accent' : 'text-fg/85 hover:bg-hover'}">
          <Icon name={t.icon} size={15} /> {t.label}
        </button>
      {/each}
      <div class="flex-1"></div>
      <div class="px-2.5 pb-1 text-[11px] leading-relaxed text-faint">Proxy <span class="mono text-muted">{proxyAddr}</span></div>
    </nav>

    <div class="flex min-w-0 flex-1 flex-col">
      <header class="flex h-12 shrink-0 items-center border-b border-line px-5">
        <div class="flex-1 text-[14px] font-semibold">{tabs.find((t) => t.id === tab)?.label}</div>
        <button onclick={close} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Close"><Icon name="x" /></button>
      </header>

      <div class="selectable min-h-0 flex-1 overflow-auto px-6 py-5 text-[12.5px] leading-relaxed text-muted">
        {#if tab === "computer"}
          <ol class="grid gap-4">
            <li class="flex items-start gap-3">
              <span class="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full {st?.ca_installed ? 'bg-ok/15 text-ok' : 'bg-accent-soft text-accent'}">{#if st?.ca_installed}<Icon name="check" size={13} />{:else}1{/if}</span>
              <div class="flex-1"><b class="text-fg">Trust the certificate.</b> {st?.ca_installed ? "ProxyZip's certificate is trusted by this computer." : "Not trusted yet. Installing it lets ProxyZip decrypt HTTPS; the key never leaves this computer."}
                {#if !st?.ca_installed}<div class="mt-2"><button onclick={installCa} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Install certificate</button></div>{/if}
              </div>
            </li>
            <li class="flex items-start gap-3">
              <span class="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full {st?.system_proxy ? 'bg-ok/15 text-ok' : 'bg-accent-soft text-accent'}">{#if st?.system_proxy}<Icon name="check" size={13} />{:else}2{/if}</span>
              <div class="flex-1"><b class="text-fg">Capture the whole computer.</b> Browsers and most apps follow the system proxy. ProxyZip restores it when you quit.
                <div class="mt-2">
                  {#if st?.system_proxy}
                    <button onclick={() => systemProxy(false)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-hover">Turn the system proxy off</button>
                  {:else}
                    <button onclick={() => systemProxy(true)} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Enable system proxy</button>
                  {/if}
                </div>
              </div>
            </li>
          </ol>
          <p class="mt-6">Apps that ignore the system proxy (many command-line tools and runtimes) need the <button class="text-accent hover:underline" onclick={() => (app.setupOpen = "terminal")}>Terminal & runtimes</button> setup. Firefox keeps its own certificates: see <button class="text-accent hover:underline" onclick={() => (app.setupOpen = "firefox")}>Firefox</button>.</p>

        {:else if tab === "terminal"}
          <p>Open a terminal that already sends its traffic through ProxyZip. curl, Node.js, Python, Ruby, Go, Git, pip, Cargo, the AWS CLI and Java trust the ProxyZip certificate there.</p>
          <div class="mt-3"><button onclick={() => app.attempt(() => api.openTerminal(), "Opened a terminal")} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90">Open a terminal</button></div>
          <div class="mt-6 flex items-center gap-3">
            <b class="text-fg">Use your own terminal</b>
            <select bind:value={shell} onchange={loadEnv} class="h-7 rounded-md border border-line bg-panel px-2 text-[12px] text-fg outline-none">
              <option value="auto">Detect</option><option value="sh">bash / zsh</option><option value="fish">fish</option><option value="powershell">PowerShell</option><option value="cmd">cmd</option>
            </select>
            <button onclick={() => copy(envText, "Paste it into a terminal")} class="flex h-7 items-center gap-1.5 rounded-md border border-line px-2.5 text-[12px] text-fg hover:bg-hover"><Icon name="copy" size={13} /> Copy</button>
          </div>
          <pre class="mono mt-3 max-h-[300px] overflow-auto rounded-lg border border-line bg-panel p-3 text-[11px] leading-snug whitespace-pre-wrap break-all text-fg/90">{envText}</pre>
          <p class="mt-3">From a shell you can also run <span class="mono text-fg">eval "$(proxyzip env)"</span>. Node.js older than 24 is covered by a small preload that tunnels <span class="mono">http</span> and <span class="mono">https</span> through the proxy.</p>

        {:else if tab === "ios"}
          <p>The device and this computer must be on the same network, and <b class="text-fg">Allow LAN connections</b> must be on in Settings.</p>
          {#if ios?.proxy}
            <div class="mt-4 flex flex-wrap gap-6">
              <ol class="grid min-w-[280px] flex-1 gap-3">
                <li><b class="text-fg">1 · Set the Wi-Fi proxy.</b> Settings → Wi-Fi → (i) → Configure Proxy → Manual. Server <span class="mono text-fg">{ios.proxy.split(":")[0]}</span>, port <span class="mono text-fg">{ios.proxy.split(":")[1]}</span>.</li>
                <li><b class="text-fg">2 · Install the profile.</b> In Safari, open <span class="mono text-fg">{ios.page}</span> (or scan the code) and tap <i>iOS profile</i>. Then Settings → Profile Downloaded → Install.</li>
                <li><b class="text-fg">3 · Trust it.</b> Settings → General → About → Certificate Trust Settings → turn on <i>ProxyZip CA</i>.</li>
                <li class="flex items-center gap-2">
                  {#if remoteClients.length}<Icon name="check" size={14} class="text-ok" /> <span class="text-ok">Traffic seen from {remoteClients.map(([n]) => n).join(", ")}</span>{:else}<span class="text-faint">Waiting for traffic from the device…</span>{/if}
                </li>
              </ol>
              {#if ios.qr_svg}
                <div class="shrink-0 text-center"><div class="rounded-xl bg-white p-2">{@html ios.qr_svg}</div><div class="mono mt-1 text-[11px] text-faint">{ios.page}</div></div>
              {/if}
            </div>
          {:else}
            <p class="mt-4 rounded-lg bg-warn/10 px-3 py-2 text-warn">Could not find this computer's network address. Connect to Wi-Fi or Ethernet and reopen this window.</p>
          {/if}
          {#if st?.platform === "macos"}
            <div class="mt-6 border-t border-line pt-4"><b class="text-fg">iOS Simulator</b> — installs the certificate into every booted simulator.
              <div class="mt-2"><button onclick={() => app.attempt(() => api.installCaIosSimulator(), "Installed into the booted iOS Simulator")} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-hover">Install into iOS Simulator</button></div>
            </div>
          {/if}

        {:else if tab === "android"}
          <div class="flex items-center gap-3"><b class="text-fg">Devices</b><button onclick={refreshDevices} class="rounded-md border border-line px-2 py-1 text-[11.5px] text-fg hover:bg-hover">Refresh</button></div>
          {#if androidError}
            <p class="mt-3 rounded-lg bg-warn/10 px-3 py-2 text-warn">{androidError}</p>
          {:else if !devices.length}
            <p class="mt-3">No device found. Start an emulator or connect a phone with USB debugging, then refresh.</p>
          {/if}
          <div class="mt-3 grid gap-3">
            {#each devices as d (d.serial)}
              <div class="rounded-xl border border-line bg-panel-2 p-4">
                <div class="flex items-center gap-2"><Icon name="smartphone" size={15} class="text-accent" /> <b class="text-fg">{d.model ?? d.serial}</b>
                  <span class="mono text-[11px] text-faint">{d.serial}</span>
                  <span class="rounded bg-hover px-1.5 text-[10.5px]">{d.emulator ? "emulator" : "device"} · {d.state}</span></div>
                {#if d.state === "device"}
                  <div class="mt-3 flex flex-wrap gap-2">
                    <button disabled={busy} onclick={() => androidProxy(d, true)} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-50">Route traffic through ProxyZip</button>
                    <button disabled={busy} onclick={() => androidProxy(d, false)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-hover disabled:opacity-50">Stop</button>
                    <button disabled={busy} onclick={() => androidCert(d, d.emulator)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-hover disabled:opacity-50">{d.emulator ? "Install as system certificate" : "Install certificate"}</button>
                    {#if d.emulator}<button disabled={busy} onclick={() => androidCert(d, false)} class="rounded-lg border border-line px-3 py-1.5 text-[12px] font-medium text-fg hover:bg-hover disabled:opacity-50">User certificate instead</button>{/if}
                  </div>
                {:else if d.state === "unauthorized"}
                  <p class="mt-2 text-warn">Accept the USB debugging prompt on the device.</p>
                {/if}
              </div>
            {/each}
          </div>
          {#if androidNote}<p class="mt-3 rounded-lg bg-accent-soft px-3 py-2 text-accent">{androidNote}</p>{/if}
          <p class="mt-5">The proxy is reached through <span class="mono">adb reverse</span>, so the device does not need to be on your network. Since Android 7, apps only trust <i>user</i> certificates if they opt in. System certificates need an emulator image without Google Play, or a rooted device.</p>

        {:else if tab === "java"}
          {#if java?.found}
            <p>ProxyZip built a copy of your JDK's trust store that also trusts its certificate. Add these options to the JVM you want to capture, or use a ProxyZip terminal where <span class="mono">JAVA_TOOL_OPTIONS</span> is already set.</p>
            <pre class="mono mt-3 overflow-auto rounded-lg border border-line bg-panel p-3 text-[11px] whitespace-pre-wrap break-all text-fg/90">{java.options}</pre>
            <div class="mt-3"><button onclick={() => copy(java?.options ?? "", "Options copied")} class="flex h-7 items-center gap-1.5 rounded-md border border-line px-2.5 text-[12px] text-fg hover:bg-hover"><Icon name="copy" size={13} /> Copy options</button></div>
            <p class="mt-4">Trust store: <span class="mono text-fg">{java.truststore}</span> ({java.store_type}, password <span class="mono">{java.password}</span>)</p>
          {:else if java}
            <p>No JDK was found. Install one or set <span class="mono">JAVA_HOME</span>, then reopen this tab.</p>
          {:else}
            <p>Looking for a JDK…</p>
          {/if}

        {:else if tab === "firefox"}
          <p>Firefox does not use the operating system's certificates by default. This adds ProxyZip to every Firefox profile on this computer and tells Firefox to trust the system store.</p>
          <div class="mt-3"><button disabled={busy} onclick={installFirefox} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-50">Set up Firefox</button></div>
          {#if firefox}
            <div class="mt-4 rounded-xl border border-line bg-panel-2 p-4">
              {#if firefox.profiles.length}<div><b class="text-fg">{firefox.profiles.length} profile{firefox.profiles.length === 1 ? "" : "s"} configured:</b> <span class="mono">{firefox.profiles.join(", ")}</span></div>{/if}
              {#each firefox.notes as note}<div class="mt-1">{note}</div>{/each}
            </div>
          {/if}
          <p class="mt-4">Firefox also needs to use the system proxy (Settings → Network Settings → <i>Use system proxy settings</i>) or the manual proxy <span class="mono text-fg">{proxyAddr}</span>.</p>
        {/if}
      </div>
    </div>
  </div>
</div>
