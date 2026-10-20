<script lang="ts">
  import { app, type Category } from "../stores/app.svelte";
  import { exportCsv, exportHar, exportPostman, exportWorkspace, importHar, importWorkspace, openSession, saveSession } from "../actions";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import Icon from "./Icon.svelte";

  const cats: { id: Category; label: string }[] = [
    { id: "all", label: "All" },
    { id: "json", label: "JSON" },
    { id: "html", label: "HTML" },
    { id: "js", label: "JS" },
    { id: "css", label: "CSS" },
    { id: "img", label: "Images" },
    { id: "font", label: "Fonts" },
    { id: "media", label: "Media" },
    { id: "ws", label: "WebSocket" },
    { id: "errors", label: "Errors" },
    { id: "other", label: "Other" },
  ];

  let menu: { x: number; y: number; items: MenuItem[] } | null = $state(null);

  /** Export everything, or just the selection when several requests are selected. */
  const scope = $derived(app.selected.size > 1 ? [...app.selected] : null);
  const scopeLabel = $derived(scope ? `${scope.length} selected` : "all requests");

  function openExportMenu(e: MouseEvent) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = {
      x: rect.right - 230,
      y: rect.bottom + 4,
      items: [
        { label: `Export ${scopeLabel} as HAR…`, icon: "download", action: () => exportHar(scope) },
        { label: `Export ${scopeLabel} as CSV…`, icon: "file", action: () => exportCsv(scope) },
        { label: `Export ${scopeLabel} as Postman collection…`, icon: "send", action: () => exportPostman(scope) },
        { separator: true },
        { label: `Save ${scopeLabel} as a session without secrets…`, icon: "shield", action: () => saveSession(scope, true) },
        { separator: true },
        { label: "Export rules and settings as a workspace…", icon: "layers", action: () => exportWorkspace() },
        { label: "Load a workspace (replace)…", icon: "upload", action: () => importWorkspace(false) },
        { label: "Load a workspace (add to current)…", icon: "plus", action: () => importWorkspace(true) },
      ],
    };
  }

  const button = "flex h-6 items-center gap-1.5 rounded-md px-2 text-[11.5px] text-muted hover:bg-hover hover:text-fg";
</script>

<div class="flex h-9 shrink-0 items-center gap-1 border-b border-line bg-panel px-2.5">
  {#each cats as c (c.id)}
    <button
      onclick={() => (app.category = c.id)}
      class="h-6 rounded-md px-2.5 text-[11.5px] font-medium transition {app.category === c.id ? 'bg-fg text-bg' : 'text-muted hover:bg-hover hover:text-fg'}"
    >
      {c.label}
    </button>
  {/each}
  <div class="flex-1"></div>
  <button onclick={openSession} class={button} title="Open a session (⌘O)"><Icon name="folderOpen" size={13} /> Open</button>
  <button onclick={() => saveSession(scope)} class={button} title="Save the session (⌘S)">
    <span class="relative"><Icon name="save" size={13} />{#if app.dirty && app.flows.length}<span class="absolute -top-0.5 -right-0.5 size-1.5 rounded-full bg-accent"></span>{/if}</span> Save
  </button>
  <span class="mx-0.5 h-4 w-px bg-line"></span>
  <button onclick={importHar} class={button} title="Import a HAR file"><Icon name="upload" size={13} /> Import</button>
  <button onclick={openExportMenu} class={button} title="Export as HAR, CSV or a Postman collection"><Icon name="download" size={13} /> Export <Icon name="chevronDown" size={11} /></button>
</div>

{#if menu}
  <ContextMenu {...menu} onclose={() => (menu = null)} />
{/if}
