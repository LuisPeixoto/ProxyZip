<script lang="ts">
  import { fly } from "svelte/transition";
  import { app } from "../stores/app.svelte";
  import Icon from "./Icon.svelte";
</script>

<div class="pointer-events-none fixed right-4 bottom-4 z-[60] flex flex-col items-end gap-2">
  {#each app.toasts as t (t.id)}
    <div
      transition:fly={{ y: 12, duration: 180 }}
      class="shadow-pop pointer-events-auto flex max-w-[460px] items-start gap-2.5 rounded-xl border border-line bg-elevated px-3.5 py-2.5 text-[12.5px]"
    >
      <Icon name={t.kind === "err" ? "alert" : t.kind === "ok" ? "check" : "info"} size={15} class="mt-px shrink-0 {t.kind === 'err' ? 'text-err' : t.kind === 'ok' ? 'text-ok' : 'text-accent'}" />
      <span class="selectable break-words">{t.text}</span>
    </div>
  {/each}
</div>
