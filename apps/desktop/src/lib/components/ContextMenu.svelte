<script lang="ts" module>
  export interface MenuItem {
    label?: string;
    icon?: string;
    shortcut?: string;
    danger?: boolean;
    disabled?: boolean;
    separator?: boolean;
    action?: () => void;
  }
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";

  let { x, y, items, onclose }: { x: number; y: number; items: MenuItem[]; onclose: () => void } = $props();
  let el: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    if (!el) return;
    const r = el.getBoundingClientRect();
    pos = {
      left: Math.min(x, window.innerWidth - r.width - 8),
      top: Math.min(y, window.innerHeight - r.height - 8),
    };
  });

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={onkey} />
<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div class="fixed inset-0 z-50" onclick={onclose} oncontextmenu={(e) => { e.preventDefault(); onclose(); }}>
  <div
    bind:this={el}
    role="menu"
    tabindex="-1"
    style="left:{pos.left}px; top:{pos.top}px"
    class="shadow-pop fixed min-w-[230px] rounded-xl border border-line bg-elevated p-1.5 backdrop-blur"
    onclick={(e) => e.stopPropagation()}
  >
    {#each items as item, i (i)}
      {#if item.separator}
        <div class="mx-2 my-1 h-px bg-line"></div>
      {:else}
        <button
          role="menuitem"
          disabled={item.disabled}
          onclick={() => {
            onclose();
            item.action?.();
          }}
          class="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[12.5px] disabled:opacity-40 {item.danger
            ? 'text-err hover:bg-err/10'
            : 'hover:bg-accent hover:text-white'}"
        >
          <Icon name={item.icon ?? "chevronRight"} size={14} class="opacity-80" />
          <span class="flex-1">{item.label}</span>
          {#if item.shortcut}<span class="text-[11px] opacity-50">{item.shortcut}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
</div>
