<script lang="ts">
  let { base64 }: { base64: string } = $props();
  const PAGE = 4096; // rows of 16 bytes
  let rowsShown = $state(PAGE);

  const bytes = $derived.by(() => {
    const bin = atob(base64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  });
  const totalRows = $derived(Math.ceil(bytes.length / 16));
  const rows = $derived.by(() => {
    const out: { off: string; hex: string; ascii: string }[] = [];
    const n = Math.min(totalRows, rowsShown);
    for (let r = 0; r < n; r++) {
      const chunk = bytes.subarray(r * 16, r * 16 + 16);
      let hex = "";
      let ascii = "";
      for (let i = 0; i < 16; i++) {
        if (i < chunk.length) {
          hex += chunk[i].toString(16).padStart(2, "0") + (i === 7 ? "  " : " ");
          ascii += chunk[i] >= 32 && chunk[i] < 127 ? String.fromCharCode(chunk[i]) : "·";
        } else hex += i === 7 ? "    " : "   ";
      }
      out.push({ off: (r * 16).toString(16).padStart(8, "0"), hex, ascii });
    }
    return out;
  });
</script>

<div class="selectable mono px-3 py-2 text-[12px] leading-[19px] whitespace-pre">
  {#each rows as r (r.off)}
    <div><span class="text-faint">{r.off}</span>  <span>{r.hex}</span> <span class="text-accent-2">{r.ascii}</span></div>
  {/each}
</div>
{#if totalRows > rowsShown}
  <button class="m-3 rounded-md border border-line px-3 py-1 text-[12px] text-accent hover:bg-hover" onclick={() => (rowsShown += PAGE * 4)}>
    Show more ({((totalRows - rowsShown) * 16).toLocaleString()} bytes left)
  </button>
{/if}
