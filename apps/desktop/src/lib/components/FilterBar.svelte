<script lang="ts">
  import { app, type Category } from "../stores/app.svelte";
  import { exportHar, importHar } from "../actions";
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
</script>

<div class="flex h-9 shrink-0 items-center gap-1 border-b border-line bg-panel px-2.5">
  {#each cats as c (c.id)}
    <button
      onclick={() => (app.category = c.id)}
      class="h-6 rounded-md px-2.5 text-[11.5px] font-medium transition {app.category === c.id
        ? 'bg-fg text-bg'
        : 'text-muted hover:bg-hover hover:text-fg'}"
    >
      {c.label}
    </button>
  {/each}
  <div class="flex-1"></div>
  <button onclick={importHar} class="flex h-6 items-center gap-1.5 rounded-md px-2 text-[11.5px] text-muted hover:bg-hover hover:text-fg" title="Import HAR">
    <Icon name="upload" size={13} /> Import
  </button>
  <button onclick={() => exportHar(null)} class="flex h-6 items-center gap-1.5 rounded-md px-2 text-[11.5px] text-muted hover:bg-hover hover:text-fg" title="Export everything as HAR">
    <Icon name="download" size={13} /> Export HAR
  </button>
</div>
