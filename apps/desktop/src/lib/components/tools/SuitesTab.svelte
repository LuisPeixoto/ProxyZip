<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Suite, type SuiteReport, type SuiteRequest } from "../../api";
  import { app } from "../../stores/app.svelte";
  import CodeEditor from "../CodeEditor.svelte";
  import Icon from "../Icon.svelte";
  import Switch from "./Switch.svelte";

  const field = "mono h-8 min-w-0 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
  const METHODS = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

  let suite = $state<Suite | null>(app.suiteDraft);
  let path = $state<string | null>(null);
  let environment = $state("");
  let viaProxy = $state(true);
  let running = $state(false);
  let report = $state<SuiteReport | null>(null);
  let expanded = $state<Record<number, boolean>>({});
  let assertionText = $state<Record<number, string>>({});
  let extractText = $state<Record<number, string>>({});
  let problems = $state<Record<string, string>>({});
  let variablesText = $state("");

  $effect(() => {
    if (app.suiteDraft) {
      suite = app.suiteDraft;
      path = null;
      report = null;
      app.suiteDraft = null;
    }
  });

  $effect(() => {
    if (suite) variablesText = JSON.stringify({ variables: suite.variables, environments: suite.environments }, null, 2);
  });

  const environments = $derived(suite ? Object.keys(suite.environments) : []);

  function blank(): Suite {
    return { format: "proxyzip-suite", version: 1, name: "New suite", variables: {}, environments: {}, requests: [] };
  }

  function addRequest() {
    suite ??= blank();
    suite.requests.push({ name: "New request", enabled: true, method: "GET", url: "https://", headers: [], body: null, assertions: [{ type: "status", class: "2xx" }], extract: [] });
  }

  async function fromSelection() {
    const ids = app.selected.size ? [...app.selected] : app.focused != null ? [app.focused] : [];
    if (!ids.length) {
      app.toast("Select some requests in the list first", "info");
      return;
    }
    const made = await app.attempt(() => api.suiteFromFlows(ids));
    if (made) {
      suite = made;
      path = null;
      report = null;
    }
  }

  async function openFile() {
    const picked = await open({ multiple: false, filters: [{ name: "Suite or Postman collection", extensions: ["pzsuite", "json"] }] });
    if (typeof picked !== "string") return;
    const loaded = await app.attempt(() => api.openSuite(picked));
    if (loaded) {
      suite = loaded;
      path = picked;
      report = null;
    }
  }

  async function saveFile() {
    if (!suite) return;
    const target = await save({ defaultPath: path ?? `${suite.name.replace(/[^\w-]+/g, "-")}.pzsuite`, filters: [{ name: "ProxyZip suite", extensions: ["pzsuite"] }] });
    if (!target) return;
    const plain = $state.snapshot(suite) as Suite;
    if ((await app.attempt(() => api.saveSuite(target, plain))) !== undefined) {
      path = target;
      app.toast("Suite saved", "ok");
    }
  }

  async function run() {
    if (!suite) return;
    running = true;
    report = null;
    try {
      const plain = $state.snapshot(suite) as Suite;
      const baseDir = path ? path.replace(/[\\/][^\\/]*$/, "") : null;
      report = await api.runSuite(plain, environment || null, viaProxy, baseDir);
    } catch (e) {
      app.toast(errorText(e), "err");
    } finally {
      running = false;
    }
  }

  async function junit() {
    if (!report) return;
    const target = await save({ defaultPath: "report.xml", filters: [{ name: "JUnit XML", extensions: ["xml"] }] });
    if (target) await app.attempt(() => api.exportSuiteJunit(target, $state.snapshot(report!) as SuiteReport), "Report saved");
  }

  function edit(key: string, text: string, apply: (value: unknown) => void) {
    try {
      apply(JSON.parse(text));
      delete problems[key];
    } catch (e) {
      problems[key] = String(e instanceof Error ? e.message : e);
    }
  }

  function applyVariables(text: string) {
    edit("vars", text, (v) => {
      const o = v as { variables?: Record<string, string>; environments?: Record<string, Record<string, string>> };
      if (suite) {
        suite.variables = o.variables ?? {};
        suite.environments = o.environments ?? {};
      }
    });
  }

  function headersText(r: SuiteRequest): string {
    return r.headers.map(([k, v]) => `${k}: ${v}`).join("\n");
  }
  function setHeaders(r: SuiteRequest, text: string) {
    r.headers = text
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .map((l) => {
        const at = l.indexOf(":");
        return (at < 0 ? [l, ""] : [l.slice(0, at).trim(), l.slice(at + 1).trim()]) as [string, string];
      });
  }

  const resultOf = (i: number) => report?.results[i];
</script>

<div class="space-y-4">
  <div>
    <h3 class="text-[15px] font-semibold">Suites</h3>
    <p class="mt-0.5 text-[12.5px] leading-relaxed text-muted">
      Replay a list of requests with variables and checks. Values from one answer (a token, an id) can feed the next request. Build a suite from the requests you
      selected, open a Postman collection, and run it here or in CI with <span class="mono">proxyzip run</span>.
    </p>
  </div>

  <div class="flex flex-wrap gap-2">
    <button type="button" onclick={fromSelection} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover"><Icon name="list" size={14} /> From selected requests</button>
    <button type="button" onclick={openFile} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover"><Icon name="folderOpen" size={14} /> Open…</button>
    <button type="button" onclick={saveFile} disabled={!suite} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover disabled:opacity-40"><Icon name="save" size={14} /> Save…</button>
    {#if !suite}<button type="button" onclick={addRequest} class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover"><Icon name="plus" size={14} /> New suite</button>{/if}
  </div>

  {#if suite}
    <div class="flex flex-wrap items-center gap-3 rounded-xl border border-line bg-panel-2 p-3 text-[12.5px]">
      <input bind:value={suite.name} aria-label="Suite name" class="{field} w-56 font-sans font-medium" />
      {#if environments.length}
        <label class="flex items-center gap-2 text-muted">Environment
          <select bind:value={environment} class="{field} w-32"><option value="">(none)</option>{#each environments as e (e)}<option value={e}>{e}</option>{/each}</select>
        </label>
      {/if}
      <label class="flex items-center gap-1.5" title="The requests appear in the list, tagged suite"><input type="checkbox" bind:checked={viaProxy} class="accent-[var(--accent)]" /> Show in the request list</label>
      <span class="flex-1"></span>
      {#if report}<button type="button" onclick={junit} class="h-8 rounded-lg border border-line px-3 font-medium hover:bg-hover">JUnit report…</button>{/if}
      <button type="button" onclick={run} disabled={running || !suite.requests.length} class="flex h-8 items-center gap-1.5 rounded-lg bg-accent px-4 font-medium text-white hover:opacity-90 disabled:opacity-50"><Icon name="play" size={13} /> {running ? "Running…" : "Run"}</button>
    </div>

    {#if report}
      <div class="flex items-center gap-3 rounded-xl border px-3 py-2 text-[12.5px] {report.failed ? 'border-err/40 bg-err/8' : 'border-ok/40 bg-ok/8'}">
        <Icon name={report.failed ? "alert" : "check"} size={15} class={report.failed ? "text-err" : "text-ok"} />
        <b>{report.passed} passed, {report.failed} failed{report.skipped ? `, ${report.skipped} skipped` : ""}</b>
        <span class="text-muted">in {report.duration_ms} ms</span>
      </div>
    {/if}

    <div class="grid gap-2">
      {#each suite.requests as r, i (i)}
        {@const res = resultOf(i)}
        <section class="rounded-xl border border-line bg-panel-2 {r.enabled ? '' : 'opacity-60'}">
          <div class="flex flex-wrap items-center gap-2 px-3 py-2">
            <Switch bind:checked={r.enabled} label="Enable request" small />
            {#if res}<Icon name={res.skipped ? "minus" : res.passed ? "check" : "x"} size={14} class={res.skipped ? "text-faint" : res.passed ? "text-ok" : "text-err"} />{/if}
            <input bind:value={r.name} aria-label="Request name" class="min-w-[120px] flex-1 bg-transparent text-[12.5px] font-medium outline-none" />
            {#if res?.status}<span class="mono text-[11.5px] {res.passed ? 'text-ok' : 'text-err'}">{res.status} · {res.duration_ms} ms</span>{/if}
            <button type="button" onclick={() => (expanded[i] = !expanded[i])} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover" aria-label="Edit the request"><Icon name={expanded[i] ? "chevronDown" : "chevronRight"} size={14} /></button>
            <button type="button" onclick={() => suite?.requests.splice(i, 1)} class="grid size-7 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err" aria-label="Delete request"><Icon name="trash" size={13} /></button>
          </div>
          {#if res && !res.passed}
            <div class="space-y-0.5 border-t border-line px-3 py-2 text-[12px] text-err">
              {#if res.error}<div>{res.error}</div>{/if}
              {#each res.assertions.filter((a) => !a.passed) as a, ai (ai)}<div>✗ {a.description}{a.message ? `: ${a.message}` : ""}</div>{/each}
            </div>
          {/if}
          {#if res && Object.keys(res.extracted).length}
            <div class="border-t border-line px-3 py-1.5 text-[11.5px] text-muted">Kept: {#each Object.entries(res.extracted) as [k, v] (k)}<span class="mono mr-3">{k} = {v.length > 40 ? v.slice(0, 40) + "…" : v}</span>{/each}</div>
          {/if}
          {#if expanded[i]}
            <div class="space-y-2 border-t border-line p-3">
              <div class="flex gap-2">
                <select bind:value={r.method} class="{field} w-28" aria-label="Method">{#each METHODS as m (m)}<option value={m}>{m}</option>{/each}</select>
                <input bind:value={r.url} spellcheck="false" placeholder={"{{vars.base}}/path"} aria-label="URL" class="{field} flex-1" />
              </div>
              <label class="block text-[11.5px] text-muted">Headers (one per line)
                <textarea value={headersText(r)} oninput={(e) => setHeaders(r, e.currentTarget.value)} rows="3" spellcheck="false" class="{field} mt-1 block h-auto w-full py-1.5"></textarea>
              </label>
              <div class="text-[11.5px] text-muted">Body
                <div class="mt-1 h-[110px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
                  <CodeEditor value={r.body ?? ""} onchange={(v) => (r.body = v || null)} readOnly={false} language="json" />
                </div>
              </div>
              <div class="grid gap-2 md:grid-cols-2">
                <div class="text-[11.5px] text-muted">Checks
                  <div class="mt-1 h-[130px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
                    <CodeEditor value={assertionText[i] ?? JSON.stringify(r.assertions, null, 2)} onchange={(v) => { assertionText[i] = v; edit(`a${i}`, v, (x) => (r.assertions = x as SuiteRequest["assertions"])); }} readOnly={false} language="json" />
                  </div>
                  {#if problems[`a${i}`]}<span class="text-err">{problems[`a${i}`]}</span>{/if}
                </div>
                <div class="text-[11.5px] text-muted">Keep for later requests
                  <div class="mt-1 h-[130px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
                    <CodeEditor value={extractText[i] ?? JSON.stringify(r.extract, null, 2)} onchange={(v) => { extractText[i] = v; edit(`e${i}`, v, (x) => (r.extract = x as SuiteRequest["extract"])); }} readOnly={false} language="json" />
                  </div>
                  {#if problems[`e${i}`]}<span class="text-err">{problems[`e${i}`]}</span>{/if}
                </div>
              </div>
              <p class="text-[11.5px] text-faint">
                Checks: <span class="mono">{'{"type":"status","equals":200}'}</span>, <span class="mono">{'{"type":"jq","expr":".id","equals":1}'}</span>, <span class="mono">{'{"type":"header","name":"x","contains":"y"}'}</span>,
                <span class="mono">body_contains</span>, <span class="mono">duration_below</span>, <span class="mono">open_api</span>. Keep: <span class="mono">{'{"var":"token","jq":".token"}'}</span>, used as <span class="mono">{"{{vars.token}}"}</span>.
              </p>
            </div>
          {/if}
        </section>
      {/each}
      <button type="button" onclick={addRequest} class="flex h-8 w-fit items-center gap-1.5 rounded-lg border border-dashed border-line px-3 text-[12px] text-muted hover:bg-hover hover:text-fg"><Icon name="plus" size={13} /> Add request</button>
    </div>

    <details class="rounded-xl border border-line bg-panel-2 p-3 text-[12.5px]">
      <summary class="cursor-pointer font-medium">Variables and environments</summary>
      <div class="mt-2 h-[150px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
        <CodeEditor value={variablesText} onchange={applyVariables} readOnly={false} language="json" />
      </div>
      {#if problems.vars}<p class="mt-1 text-err">{problems.vars}</p>{/if}
      <p class="mt-1 text-[11.5px] text-faint">Use them as <span class="mono">{"{{vars.name}}"}</span>. An environment overrides the variables of the same name.</p>
    </details>
  {:else}
    <p class="rounded-xl border border-dashed border-line px-4 py-6 text-center text-[12.5px] text-faint">No suite open. Select requests in the list and press “From selected requests”, or open a file.</p>
  {/if}
</div>
