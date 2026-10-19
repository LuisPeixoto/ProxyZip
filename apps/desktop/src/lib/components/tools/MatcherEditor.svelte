<script lang="ts">
  import type { Matcher } from "../../api";

  let { matcher = $bindable(), placeholder = "api.example.com/v1/*" }: { matcher: Matcher; placeholder?: string } = $props();

  function setMethods(text: string) {
    matcher.methods = text
      .split(/[\s,]+/)
      .map((m) => m.trim().toUpperCase())
      .filter(Boolean);
  }
</script>

<div class="space-y-2">
  <div class="flex gap-2">
    <input
      bind:value={matcher.pattern}
      spellcheck="false"
      placeholder={matcher.regex ? String.raw`/users/\d+$` : placeholder}
      aria-label="URL pattern"
      class="selectable mono h-8 min-w-0 flex-1 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent"
    />
    <input
      value={matcher.methods.join(", ")}
      oninput={(e) => setMethods(e.currentTarget.value)}
      spellcheck="false"
      placeholder="Any method"
      aria-label="Methods"
      class="mono h-8 w-36 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent"
    />
    <label class="flex h-8 items-center gap-1.5 rounded-lg border border-line px-2 text-[11.5px] text-muted" title="Treat the pattern as a regular expression">
      <input type="checkbox" bind:checked={matcher.regex} class="accent-[var(--accent)]" /> Regex
    </label>
  </div>
  <p class="text-[11px] text-faint">
    {#if matcher.regex}
      Case-insensitive regular expression, tested against the full URL.
    {:else}
      <span class="mono">*</span> matches anything and <span class="mono">?</span> one character. A bare host such as
      <span class="mono">example.com</span> matches every request to it. Leave empty to match everything.
    {/if}
  </p>
</div>
