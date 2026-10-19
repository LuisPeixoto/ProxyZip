<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type LanguageInfo } from "../api";
  import { copy } from "../actions";
  import { app } from "../stores/app.svelte";
  import type { EditorLanguage } from "../editor";
  import CodeEditor from "./CodeEditor.svelte";
  import Icon from "./Icon.svelte";

  const STORAGE_KEY = "pz.codeLanguage";
  let languages = $state<LanguageInfo[]>([]);
  let selected = $state(readSaved());
  let code = $state("");
  let failure = $state<string | null>(null);
  const flow = $derived(app.codeFor != null ? app.get(app.codeFor) : undefined);
  const info = $derived(languages.find((l) => l.id === selected));

  function readSaved(): string {
    try {
      return localStorage.getItem(STORAGE_KEY) ?? "curl";
    } catch {
      return "curl";
    }
  }

  onMount(async () => {
    languages = await api.codeLanguages();
  });

  $effect(() => {
    const id = app.codeFor;
    const language = selected;
    if (id == null) return;
    api
      .generateCode(id, language)
      .then((text) => {
        code = text;
        failure = null;
      })
      .catch((e) => (failure = errorText(e)));
  });

  function choose(id: string) {
    selected = id;
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch {
      /* storage unavailable */
    }
  }

  function close() {
    app.codeFor = null;
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={close}>
  <div class="shadow-pop flex h-full max-h-[700px] w-full max-w-[1000px] overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Generate code" tabindex="-1">
    <nav class="flex w-[210px] shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-line bg-panel-2 p-2.5">
      <div class="flex items-center gap-2 px-2.5 pt-1.5 pb-3 text-[14px] font-semibold"><Icon name="code" size={16} /> Code</div>
      {#each languages as l (l.id)}
        <button onclick={() => choose(l.id)} class="flex h-8 items-center rounded-lg px-2.5 text-left text-[12.5px] {selected === l.id ? 'bg-accent-soft font-medium text-accent' : 'text-fg/85 hover:bg-hover'}">{l.label}</button>
      {/each}
    </nav>
    <div class="flex min-w-0 flex-1 flex-col">
      <header class="flex h-12 shrink-0 items-center gap-3 border-b border-line px-4">
        <div class="min-w-0 flex-1">
          <div class="text-[13px] font-semibold">{info?.label ?? ""}</div>
          <div class="mono truncate text-[11px] text-muted">{flow ? `${flow.method} ${flow.url}` : ""}</div>
        </div>
        <button onclick={() => copy(code, "Code copied")} class="flex h-8 items-center gap-1.5 rounded-lg bg-accent px-3 text-[12.5px] font-medium text-white hover:opacity-90"><Icon name="copy" size={14} /> Copy</button>
        <button onclick={close} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Close"><Icon name="x" /></button>
      </header>
      {#if failure}
        <p class="selectable m-4 rounded-lg bg-err/8 px-3 py-2 text-[12px] text-err">{failure}</p>
      {:else}
        <div class="min-h-0 flex-1"><CodeEditor value={code} language={(info?.syntax ?? "text") as EditorLanguage} readOnly wrap /></div>
      {/if}
    </div>
  </div>
</div>
