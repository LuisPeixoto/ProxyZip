<script lang="ts">
  import { api, errorText, newMatcher, type RuleSet, type ScriptPhaseResult, type ScriptTestResult } from "../../api";
  import { app } from "../../stores/app.svelte";
  import CodeEditor from "../CodeEditor.svelte";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  const TEMPLATE = `// Runs for every matching request. Edit \`request\` and return it, or return null to block the request.
function onRequest(ctx, request) {
  request.headers["x-debug"] = "proxyzip";
  return request;
}

// Runs for every matching response once it has been fully received.
// \`response.body\` is the decoded text. Change it and return the response.
function onResponse(ctx, request, response) {
  return response;
}
`;

  let results = $state<Record<number, ScriptTestResult | string>>({});
  let testing = $state<number | null>(null);
  const selected = $derived(app.focused != null ? app.get(app.focused) : undefined);

  function add() {
    rules.scripts.push({ name: "", enabled: true, matcher: newMatcher(), source: TEMPLATE, on_request: true, on_response: true });
  }

  async function test(index: number) {
    if (app.focused == null) return;
    testing = index;
    try {
      results[index] = await api.testScript(rules.scripts[index].source, app.focused);
    } catch (e) {
      results[index] = errorText(e);
    } finally {
      testing = null;
    }
  }

  function outcomeLabel(r: ScriptPhaseResult | null): { text: string; cls: string } {
    if (!r) return { text: "no response to test", cls: "text-faint" };
    switch (r.outcome) {
      case "changed":
        return { text: "changed", cls: "text-ok" };
      case "block":
        return { text: "blocks the request", cls: "text-warn" };
      case "error":
        return { text: "error", cls: "text-err" };
      default:
        return { text: "no change", cls: "text-muted" };
    }
  }
</script>

<TabFrame
  title="Scripts"
  description="Write JavaScript that edits requests and responses as they pass through. Scripts are sandboxed (no file or network access), stopped after 2 seconds, and a failing script never breaks your traffic."
  bind:enabled={rules.tools.scripts}
  onadd={add}
  addLabel="Add script"
  count={rules.scripts.length}
  empty="No scripts yet."
>
  <details class="rounded-xl border border-line bg-panel-2 p-3 text-[12.5px]">
    <summary class="flex cursor-pointer items-center gap-2 font-medium"><Icon name="book" size={14} /> API reference</summary>
    <div class="mt-3 space-y-2 leading-relaxed text-muted">
      <p><span class="mono text-fg">request</span>: <span class="mono">method, url, headers, body, bodyBase64</span> · <span class="mono text-fg">response</span>: <span class="mono">status, headers, body, bodyBase64</span>. Headers are an object; a repeated header is an array. <span class="mono">body</span> is the decoded text, or <span class="mono">null</span> for binary data (use <span class="mono">bodyBase64</span>).</p>
      <p>Return the object (or nothing, if you edited it in place). Return <span class="mono">null</span> from <span class="mono">onRequest</span> to block the request. <span class="mono text-fg">ctx.shared</span> is an object that persists between runs of the same script.</p>
      <p><span class="mono text-fg">console.log / warn / error</span> appear in the request's Console tab. Helpers: <span class="mono">$.base64.encode/decode, $.md5, $.sha1, $.sha256, $.hmacSha256(key, text), $.uuid(), $.jwt.decode(token), $.json.parse/stringify, $.sleep(ms)</span>.</p>
      <p>Functions must be synchronous. Full guide: <span class="mono">docs/scripting.md</span>.</p>
    </div>
  </details>

  {#each rules.scripts as rule, i (i)}
    {@const result = results[i]}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.scripts.splice(i, 1)} placeholder="Script">
      <MatcherEditor bind:matcher={rule.matcher} />
      <div class="flex items-center gap-5 text-[12.5px]">
        <span class="text-muted">Run on the</span>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={rule.on_request} class="accent-[var(--accent)]" /> request</label>
        <label class="flex items-center gap-1.5"><input type="checkbox" bind:checked={rule.on_response} class="accent-[var(--accent)]" /> response</label>
      </div>
      <div class="h-[260px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
        <CodeEditor value={rule.source} onchange={(v) => (rule.source = v)} readOnly={false} language="javascript" />
      </div>
      <div class="flex items-center gap-3">
        <button
          type="button"
          onclick={() => test(i)}
          disabled={!selected || testing === i}
          title={selected ? "Run this script against the selected request. Nothing is sent." : "Select a request in the list first"}
          class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-3 text-[12.5px] font-medium hover:bg-hover disabled:opacity-40"
        >
          <Icon name="flask" size={14} /> {testing === i ? "Running…" : "Test with the selected request"}
        </button>
        {#if selected}<span class="mono min-w-0 truncate text-[11px] text-faint">{selected.method} {selected.host}{selected.path}</span>{/if}
      </div>
      {#if typeof result === "string"}
        <p class="selectable rounded-lg bg-err/8 px-3 py-2 text-[12px] text-err">{result}</p>
      {:else if result}
        {@const req = outcomeLabel(result.request)}
        {@const res = outcomeLabel(result.response)}
        <div class="space-y-2 rounded-xl border border-line bg-panel p-3 text-[12.5px]">
          <div class="flex gap-6">
            <span><span class="text-muted">onRequest:</span> <b class={req.cls}>{req.text}</b></span>
            <span><span class="text-muted">onResponse:</span> <b class={res.cls}>{res.text}</b></span>
          </div>
          {#each [["request", result.request], ["response", result.response]] as [label, phase] (label)}
            {#if phase && typeof phase === "object" && "outcome" in phase}
              {#if phase.error}<p class="selectable mono text-[12px] break-words text-err">{label}: {phase.error}</p>{/if}
              {#if phase.outcome === "changed"}
                <div class="text-[11px] font-semibold tracking-wide text-muted uppercase">Resulting {label}</div>
                <div class="h-[150px] overflow-hidden rounded-lg border border-line"><CodeEditor value={JSON.stringify(phase.value, null, 2)} language="json" readOnly /></div>
              {/if}
            {/if}
          {/each}
          {#if result.logs.length}
            <div class="text-[11px] font-semibold tracking-wide text-muted uppercase">Console</div>
            <div class="selectable mono space-y-0.5 text-[12px]">
              {#each result.logs as line, n (n)}<div class={line.level === "error" ? "text-err" : line.level === "warn" ? "text-warn" : ""}>{line.message}</div>{/each}
            </div>
          {/if}
        </div>
      {/if}
    </RuleCard>
  {/each}
</TabFrame>
