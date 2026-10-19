<script lang="ts">
  let { text, language = "text", wrap = false }: { text: string; language?: "json" | "text"; wrap?: boolean } = $props();
  const LIMIT = 400_000;
  let showAll = $state(false);
  const shown = $derived(showAll ? text : text.slice(0, LIMIT));
  const lines = $derived(shown.split("\n"));

  function esc(s: string) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // Tiny JSON highlighter; runs per line so huge documents stay cheap to render.
  function highlight(line: string): string {
    return esc(line).replace(
      /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false)\b|\bnull\b|-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b|[{}[\],]/g,
      (m, str, colon, bool) => {
        if (str) return colon ? `<span class="tok-key">${str}</span><span class="tok-punct">${colon}</span>` : `<span class="tok-str">${str}</span>`;
        if (bool) return `<span class="tok-bool">${m}</span>`;
        if (m === "null") return `<span class="tok-null">${m}</span>`;
        if (/^[{}[\],]$/.test(m)) return `<span class="tok-punct">${m}</span>`;
        return `<span class="tok-num">${m}</span>`;
      },
    );
  }
</script>

<div class="selectable mono flex text-[12px] leading-[19px]">
  <div class="sticky left-0 shrink-0 border-r border-line bg-panel pr-2 pl-3 text-right text-faint select-none" aria-hidden="true">
    {#each lines as _, i (i)}<div>{i + 1}</div>{/each}
  </div>
  <pre class="m-0 min-w-0 flex-1 px-3 {wrap ? 'break-all whitespace-pre-wrap' : 'whitespace-pre'}">{#if language === "json"}{#each lines as l, i (i)}<div>{@html highlight(l) || " "}</div>{/each}{:else}{shown}{/if}</pre>
</div>
{#if text.length > LIMIT && !showAll}
  <button class="m-3 rounded-md border border-line px-3 py-1 text-[12px] text-accent hover:bg-hover" onclick={() => (showAll = true)}>
    Showing first {(LIMIT / 1000).toFixed(0)} KB — show everything ({(text.length / 1000).toFixed(0)} KB)
  </button>
{/if}
