<script lang="ts">
  import { newMatcher, type RewriteOp, type RuleSet } from "../../api";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  const labels: Record<RewriteOp["type"], string> = {
    set_header: "Set header",
    add_header: "Add header",
    remove_header: "Remove header",
    set_query: "Set query parameter",
    remove_query: "Remove query parameter",
    set_status: "Set status",
    replace_body: "Replace in body",
  };
  const requestOnly: RewriteOp["type"][] = ["set_query", "remove_query"];
  const responseOnly: RewriteOp["type"][] = ["set_status"];

  function defaults(type: RewriteOp["type"]): RewriteOp {
    switch (type) {
      case "set_header":
      case "add_header":
      case "set_query":
        return { type, name: "", value: "" };
      case "remove_header":
      case "remove_query":
        return { type, name: "" };
      case "set_status":
        return { type, status: 200 };
      case "replace_body":
        return { type, find: "", replace: "", regex: false };
    }
  }

  function add() {
    rules.rewrite.push({ name: "", enabled: true, matcher: newMatcher(), phase: "response", ops: [{ type: "set_header", name: "", value: "" }] });
  }

  function allowed(phase: "request" | "response") {
    return (Object.keys(labels) as RewriteOp["type"][]).filter((t) => (phase === "request" ? !responseOnly.includes(t) : !requestOnly.includes(t)));
  }

  const field = "mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
</script>

<TabFrame
  title="Rewrite"
  description="Edit matching requests or responses on the fly: headers, query parameters, status codes and body text. Body edits see decompressed text."
  bind:enabled={rules.tools.rewrite}
  onadd={add}
  addLabel="Add rewrite rule"
  count={rules.rewrite.length}
  empty="No rewrite rules yet."
>
  {#each rules.rewrite as rule, i (i)}
    <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.rewrite.splice(i, 1)} placeholder="Rewrite rule">
      <MatcherEditor bind:matcher={rule.matcher} />
      <div class="flex items-center gap-2 text-[12.5px]">
        <span class="text-muted">Apply to the</span>
        <select
          bind:value={rule.phase}
          onchange={() => (rule.ops = rule.ops.filter((op) => allowed(rule.phase).includes(op.type)))}
          class="h-8 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent"
        >
          <option value="request">request</option>
          <option value="response">response</option>
        </select>
      </div>
      <div class="space-y-2">
        {#each rule.ops as op, j (j)}
          <div class="flex items-center gap-2 text-[12.5px]">
            <select value={op.type} onchange={(e) => (rule.ops[j] = defaults(e.currentTarget.value as RewriteOp["type"]))} class="h-8 w-44 shrink-0 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent">
              {#each allowed(rule.phase) as t (t)}<option value={t}>{labels[t]}</option>{/each}
            </select>
            {#if op.type === "set_header" || op.type === "add_header" || op.type === "set_query"}
              <input bind:value={op.name} placeholder="name" spellcheck="false" class={field} />
              <input bind:value={op.value} placeholder="value" spellcheck="false" class={field} />
            {:else if op.type === "remove_header" || op.type === "remove_query"}
              <input bind:value={op.name} placeholder="name" spellcheck="false" class={field} />
            {:else if op.type === "set_status"}
              <input type="number" min="100" max="599" bind:value={op.status} class="mono h-8 w-24 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent" />
            {:else}
              <input bind:value={op.find} placeholder={op.regex ? "pattern" : "find"} spellcheck="false" class={field} />
              <input bind:value={op.replace} placeholder="replace with" spellcheck="false" class={field} />
              <label class="flex items-center gap-1 text-[11.5px] text-muted"><input type="checkbox" bind:checked={op.regex} class="accent-[var(--accent)]" /> Regex</label>
            {/if}
            <button type="button" onclick={() => rule.ops.splice(j, 1)} title="Remove step" class="grid size-6 shrink-0 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err"><Icon name="x" size={13} /></button>
          </div>
        {/each}
        <button type="button" onclick={() => rule.ops.push(defaults(allowed(rule.phase)[0]))} class="flex h-7 items-center gap-1 rounded-md px-2 text-[12px] text-accent hover:bg-accent-soft"><Icon name="plus" size={13} /> Add step</button>
      </div>
    </RuleCard>
  {/each}
</TabFrame>
