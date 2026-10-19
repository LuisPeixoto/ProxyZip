<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { api, errorText, newMatcher, type ProtoInfo, type RuleSet } from "../../api";
  import { app } from "../../stores/app.svelte";
  import Icon from "../Icon.svelte";
  import MatcherEditor from "./MatcherEditor.svelte";
  import RuleCard from "./RuleCard.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();
  let info = $state<ProtoInfo>({ files: [], messages: [], methods: [] });
  let showMethods = $state(false);

  onMount(async () => {
    info = await api.listProtos();
  });

  async function importSchema() {
    const chosen = await open({ multiple: true, filters: [{ name: "Descriptor set", extensions: ["desc", "pb", "protoset", "bin"] }] });
    const paths = chosen == null ? [] : Array.isArray(chosen) ? chosen : [chosen];
    for (const path of paths) {
      try {
        info = await api.importProto(path);
        app.toast(`Imported ${path.split(/[\\/]/).pop()}`, "ok");
      } catch (e) {
        app.toast(errorText(e), "err");
      }
    }
  }

  async function remove(name: string) {
    try {
      info = await api.removeProto(name);
    } catch (e) {
      app.toast(errorText(e), "err");
    }
  }

  function add() {
    rules.proto_mappings.push({ name: "", enabled: true, matcher: newMatcher(), request_type: null, response_type: null });
  }
  const field = "mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
</script>

<div class="space-y-8">
  <TabFrame
    title="Protobuf schemas"
    description="Without a schema, Protobuf and gRPC bodies show field numbers. Import a descriptor set and they show real field names. gRPC calls are matched to their method automatically."
  >
    <div class="rounded-xl border border-line bg-panel-2 p-4">
      <div class="mb-3 flex items-center justify-between">
        <span class="text-[12.5px] font-medium">{info.files.length} schema file{info.files.length === 1 ? "" : "s"} · {info.messages.length} messages · {info.methods.length} gRPC methods</span>
        <button type="button" onclick={importSchema} class="flex h-8 items-center gap-1.5 rounded-lg bg-accent px-3 text-[12.5px] font-medium text-white hover:opacity-90"><Icon name="upload" size={14} /> Import descriptor set…</button>
      </div>
      {#each info.files as file (file)}
        <div class="flex items-center gap-2 border-t border-line py-1.5 text-[12.5px]">
          <Icon name="file" size={14} class="text-muted" />
          <span class="mono flex-1 truncate">{file}</span>
          <button type="button" onclick={() => remove(file)} title="Remove" class="grid size-6 place-items-center rounded-md text-faint hover:bg-err/10 hover:text-err"><Icon name="trash" size={13} /></button>
        </div>
      {/each}
      {#if info.methods.length}
        <button type="button" onclick={() => (showMethods = !showMethods)} class="mt-2 text-[12px] text-accent hover:underline">{showMethods ? "Hide" : "Show"} gRPC methods</button>
        {#if showMethods}
          <div class="selectable mono mt-2 max-h-40 overflow-auto rounded-lg border border-line bg-panel p-2 text-[11.5px]">
            {#each info.methods as m (m)}<div>/{m}</div>{/each}
          </div>
        {/if}
      {/if}
      <p class="mt-3 text-[11.5px] leading-relaxed text-faint">
        Create one with <span class="mono rounded bg-hover px-1">protoc --include_imports --descriptor_set_out=schema.desc your.proto</span>.
        Files are copied into your ProxyZip data folder.
      </p>
    </div>
  </TabFrame>

  <TabFrame
    title="Message types for plain Protobuf endpoints"
    description="gRPC needs no setup. For a REST-style endpoint that sends Protobuf, say which message types it uses."
    onadd={add}
    addLabel="Add mapping"
    count={rules.proto_mappings.length}
    empty="No mappings yet."
  >
    <datalist id="proto-messages">{#each info.messages as m (m)}<option value={m}></option>{/each}</datalist>
    {#each rules.proto_mappings as rule, i (i)}
      <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.proto_mappings.splice(i, 1)} placeholder="Protobuf mapping">
        <MatcherEditor bind:matcher={rule.matcher} placeholder="api.example.com/v1/events" />
        <div class="grid grid-cols-2 gap-2 text-[12.5px]">
          <label class="flex flex-col gap-1"><span class="text-muted">Request type</span><input bind:value={rule.request_type} list="proto-messages" spellcheck="false" placeholder="demo.EventRequest" class={field} /></label>
          <label class="flex flex-col gap-1"><span class="text-muted">Response type</span><input bind:value={rule.response_type} list="proto-messages" spellcheck="false" placeholder="demo.EventReply" class={field} /></label>
        </div>
      </RuleCard>
    {/each}
  </TabFrame>
</div>
