<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type ContractReport, type MockRoute, type RuleSet } from "../../api";
  import { app } from "../../stores/app.svelte";
  import CodeEditor from "../CodeEditor.svelte";
  import Icon from "../Icon.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  const field = "mono h-8 min-w-0 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
  const METHODS = ["", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

  let report = $state<ContractReport | null>(null);
  let busy = $state(false);
  let expanded = $state<Record<string, boolean>>({});

  function addServer() {
    rules.mocks.push({ name: "", enabled: true, host: "api.mock", listen_port: null, passthrough: false, routes: [] });
  }

  function addRoute(server: number) {
    rules.mocks[server].routes.push({
      name: "",
      enabled: true,
      method: "GET",
      path: "/",
      status: 200,
      headers: [["content-type", "application/json"]],
      body: { type: "inline", text: '{ "ok": true }' },
      template: false,
      delay_ms: 0,
    });
  }

  function bodyText(route: MockRoute): string {
    return route.body.type === "inline" ? route.body.text : "";
  }

  async function importSpec() {
    const path = await open({ multiple: false, filters: [{ name: "OpenAPI", extensions: ["yaml", "yml", "json"] }] });
    if (typeof path !== "string") return;
    busy = true;
    try {
      const imported = await api.importOpenapi(path, null);
      // The command saved and applied the rules; take them over so the editor shows the new server.
      rules.mocks = (await api.getRules()).mocks;
      rules.tools.mocks = true;
      app.rules = await api.getRules();
      app.toast(`Added “${imported.title}” with ${imported.routes} routes`, "ok");
    } catch (e) {
      app.toast(errorText(e), "err");
    } finally {
      busy = false;
    }
  }

  async function validate() {
    const path = await open({ multiple: false, filters: [{ name: "OpenAPI", extensions: ["yaml", "yml", "json"] }] });
    if (typeof path !== "string") return;
    busy = true;
    try {
      const ids = app.selected.size > 1 ? [...app.selected] : null;
      report = await api.validateOpenapi(path, ids);
    } catch (e) {
      app.toast(errorText(e), "err");
    } finally {
      busy = false;
    }
  }

  function show(id: number) {
    app.select(id);
    app.toolsOpen = false;
  }
</script>

<TabFrame
  title="Mocks"
  description="Answer requests with your own responses, no server needed. Mock a host (api.mock), or give a mock its own port and call http://localhost:PORT. Import an OpenAPI file to get every route at once."
  bind:enabled={rules.tools.mocks}
  onadd={addServer}
  addLabel="Add mock server"
  count={rules.mocks.length}
  empty="No mock servers yet."
>
  <div class="flex flex-wrap gap-2">
    <button type="button" onclick={importSpec} disabled={busy} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover disabled:opacity-50"><Icon name="upload" size={14} /> Import OpenAPI…</button>
    <button type="button" onclick={validate} disabled={busy} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover disabled:opacity-50" title="Check {app.selected.size > 1 ? 'the selected requests' : 'every captured request'} against an OpenAPI file"><Icon name="shieldCheck" size={14} /> Check traffic against an OpenAPI file…</button>
  </div>

  {#if report}
    <section class="rounded-xl border border-line bg-panel-2 p-3 text-[12.5px]">
      <div class="flex items-center gap-2">
        <Icon name={report.violations.length ? "alert" : "check"} size={15} class={report.violations.length ? "text-warn" : "text-ok"} />
        <b>{report.title}</b>
        <span class="text-muted">{report.checked} checked, {report.skipped} outside the API, {report.violations.length} problem{report.violations.length === 1 ? "" : "s"}</span>
        <span class="flex-1"></span>
        <button type="button" onclick={() => (report = null)} class="text-faint hover:text-fg" aria-label="Dismiss"><Icon name="x" size={14} /></button>
      </div>
      {#if report.violations.length}
        <ul class="mt-2 grid gap-1.5">
          {#each report.violations as v, i (i)}
            <li>
              <button type="button" onclick={() => show(v.flow_id)} class="w-full rounded-lg px-2 py-1.5 text-left hover:bg-hover">
                <span class="mono text-[11.5px] text-muted">#{v.flow_id} {v.method} {v.url}</span>
                <span class="block text-err">{v.message}{v.pointer ? ` (at ${v.pointer})` : ""}</span>
              </button>
            </li>
          {/each}
        </ul>
        <p class="mt-2 text-[11.5px] text-faint">The affected requests are tagged <span class="mono">contract-violation</span>.</p>
      {/if}
    </section>
  {/if}

  {#each rules.mocks as server, si (si)}
    <RuleCard bind:name={server.name} bind:enabled={server.enabled} ondelete={() => rules.mocks.splice(si, 1)} placeholder="Mock server">
      <div class="flex flex-wrap items-center gap-3 text-[12.5px]">
        <label class="flex items-center gap-2 text-muted">Host <input bind:value={server.host} placeholder="api.mock or api.example.com" spellcheck="false" class="{field} w-56" /></label>
        <label class="flex items-center gap-2 text-muted">Own port
          <input type="number" min="1" max="65535" placeholder="none" value={server.listen_port ?? ""} oninput={(e) => (server.listen_port = e.currentTarget.value ? Number(e.currentTarget.value) : null)} class="{field} w-24" />
        </label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={server.passthrough} class="accent-[var(--accent)]" /> Send unmatched requests to the real server</label>
      </div>

      <div class="grid gap-2">
        {#each server.routes as route, ri (ri)}
          {@const key = `${si}-${ri}`}
          <div class="rounded-lg border border-line bg-panel {route.enabled ? '' : 'opacity-60'}">
            <div class="flex flex-wrap items-center gap-2 p-2">
              <input type="checkbox" bind:checked={route.enabled} class="accent-[var(--accent)]" aria-label="Enable route" />
              <select bind:value={route.method} class="{field} w-24" aria-label="Method">
                {#each METHODS as m (m)}<option value={m}>{m || "ANY"}</option>{/each}
              </select>
              <input bind:value={route.path} placeholder="/users/:id" spellcheck="false" class="{field} min-w-[160px] flex-1" aria-label="Path" />
              <input type="number" min="100" max="599" bind:value={route.status} class="{field} w-20" aria-label="Status" />
              <label class="flex items-center gap-1 text-[11.5px] text-muted"><input type="number" min="0" bind:value={route.delay_ms} class="{field} w-20" aria-label="Delay" /> ms</label>
              <button type="button" onclick={() => (expanded[key] = !expanded[key])} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover" aria-label="Edit the response"><Icon name={expanded[key] ? "chevronDown" : "chevronRight"} size={14} /></button>
              <button type="button" onclick={() => server.routes.splice(ri, 1)} class="grid size-7 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err" aria-label="Delete route"><Icon name="trash" size={13} /></button>
            </div>
            {#if expanded[key]}
              <div class="space-y-2 border-t border-line p-2">
                <div class="flex flex-wrap items-center gap-3 text-[12px]">
                  <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={route.template} class="accent-[var(--accent)]" /> Fill in <span class="mono">{"{{params.id}}"}</span>, <span class="mono">{"{{body.name}}"}</span>, <span class="mono">{"{{uuid}}"}</span>…</label>
                  <span class="flex-1"></span>
                  <label class="flex items-center gap-2 text-muted">Content type
                    <input
                      value={route.headers.find(([k]) => k.toLowerCase() === "content-type")?.[1] ?? ""}
                      oninput={(e) => {
                        const v = e.currentTarget.value;
                        const at = route.headers.findIndex(([k]) => k.toLowerCase() === "content-type");
                        if (at >= 0) route.headers[at][1] = v;
                        else route.headers.push(["content-type", v]);
                      }}
                      class="{field} w-52"
                    />
                  </label>
                </div>
                <div class="h-[180px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
                  <CodeEditor value={bodyText(route)} onchange={(v) => (route.body = { type: "inline", text: v })} readOnly={false} language="json" />
                </div>
              </div>
            {/if}
          </div>
        {/each}
        <button type="button" onclick={() => addRoute(si)} class="flex h-8 w-fit items-center gap-1.5 rounded-lg border border-dashed border-line px-3 text-[12px] text-muted hover:bg-hover hover:text-fg"><Icon name="plus" size={13} /> Add route</button>
      </div>
    </RuleCard>
  {/each}
</TabFrame>
