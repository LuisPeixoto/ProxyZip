<script lang="ts">
  import type { FlowDetail } from "../api";
  import { formatDuration } from "../api";

  let { detail }: { detail: FlowDetail } = $props();
  const t = $derived(detail.timing);
  const end = $derived(t.finished ?? Date.now());
  const total = $derived(Math.max(1, end - t.started));
  const phases = $derived.by(() => {
    const out: { label: string; from: number; to: number; color: string }[] = [];
    const sent = t.request_sent ?? t.started;
    out.push({ label: "Request", from: t.started, to: sent, color: "bg-info" });
    if (t.response_started) {
      out.push({ label: "Waiting (TTFB)", from: sent, to: t.response_started, color: "bg-warn" });
      out.push({ label: "Download", from: t.response_started, to: end, color: "bg-ok" });
    } else {
      out.push({ label: "Waiting", from: sent, to: end, color: "bg-warn" });
    }
    return out;
  });
</script>

<div class="space-y-4 p-4 text-[12px]">
  <div class="flex items-baseline justify-between">
    <span class="text-muted">Total</span>
    <span class="mono text-[18px] font-semibold">{formatDuration(t.finished ? total : null) || "in progress"}</span>
  </div>
  <div class="space-y-2.5">
    {#each phases as p (p.label)}
      <div class="grid grid-cols-[120px_1fr_70px] items-center gap-3">
        <span class="text-muted">{p.label}</span>
        <div class="relative h-2.5 rounded-full bg-hover">
          <div
            class="absolute h-full rounded-full {p.color}"
            style="left:{((p.from - t.started) / total) * 100}%; width:{Math.max(0.6, ((p.to - p.from) / total) * 100)}%"
          ></div>
        </div>
        <span class="mono text-right">{formatDuration(Math.max(0, p.to - p.from))}</span>
      </div>
    {/each}
  </div>
  <div class="grid grid-cols-2 gap-x-6 gap-y-1.5 border-t border-line pt-3 text-[11.5px]">
    <span class="text-muted">Started</span><span class="mono selectable">{new Date(t.started).toLocaleString()}</span>
    <span class="text-muted">Client</span><span class="mono selectable">{detail.summary.client_addr}</span>
    <span class="text-muted">Protocol</span><span class="mono">{detail.summary.http_version}</span>
  </div>
  {#if detail.tls}
    <div class="grid grid-cols-2 gap-x-6 gap-y-1.5 border-t border-line pt-3 text-[11.5px]">
      <span class="font-medium text-fg">TLS (client ↔ ProxyZip)</span><span></span>
      <span class="text-muted">Version</span><span class="mono">{detail.tls.version ?? "—"}</span>
      <span class="text-muted">Cipher</span><span class="mono break-all">{detail.tls.cipher ?? "—"}</span>
      <span class="text-muted">SNI</span><span class="mono selectable">{detail.tls.sni ?? "—"}</span>
      <span class="text-muted">ALPN</span><span class="mono">{detail.tls.alpn ?? "—"}</span>
    </div>
  {/if}
</div>
