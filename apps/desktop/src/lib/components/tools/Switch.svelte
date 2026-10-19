<script lang="ts">
  let { checked = $bindable(false), label = "", small = false }: { checked?: boolean; label?: string; small?: boolean } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  title={label}
  onclick={() => (checked = !checked)}
  class="relative shrink-0 rounded-full transition-colors {checked ? 'bg-accent' : 'bg-line-strong'} {small ? 'h-[18px] w-8' : 'h-5 w-9'}"
>
  <span
    class="absolute top-0.5 left-0.5 rounded-full bg-white shadow transition-transform {small ? 'size-[14px]' : 'size-4'} {checked ? (small ? 'translate-x-[14px]' : 'translate-x-4') : ''}"
  ></span>
</button>
