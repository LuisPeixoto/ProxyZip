<script lang="ts">
  import type { RuleSet } from "../../api";
  import Icon from "../Icon.svelte";
  import RuleCard from "./RuleCard.svelte";
  import Switch from "./Switch.svelte";
  import TabFrame from "./TabFrame.svelte";

  let { rules = $bindable() }: { rules: RuleSet } = $props();

  const field = "mono h-8 min-w-0 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent";
  const bypassText = $derived(rules.upstream.bypass.join("\n"));
</script>

<div class="space-y-8">
  <TabFrame
    title="Upstream proxy"
    description="Chain ProxyZip through another proxy, such as a corporate gateway. HTTP proxies must allow CONNECT. Changes apply to new connections."
    bind:enabled={rules.upstream.enabled}
  >
    <div class="grid grid-cols-[120px_1fr_100px] gap-2 text-[12.5px]">
      <label class="flex flex-col gap-1"><span class="text-muted">Type</span>
        <select bind:value={rules.upstream.kind} class="h-8 rounded-lg border border-line bg-panel px-2 outline-none focus:border-accent">
          <option value="http">HTTP</option><option value="socks5">SOCKS5</option>
        </select>
      </label>
      <label class="flex flex-col gap-1"><span class="text-muted">Host</span><input bind:value={rules.upstream.host} spellcheck="false" placeholder="proxy.example.com" class={field} /></label>
      <label class="flex flex-col gap-1"><span class="text-muted">Port</span><input type="number" min="1" max="65535" bind:value={rules.upstream.port} class={field} /></label>
    </div>
    <div class="grid grid-cols-2 gap-2 text-[12.5px]">
      <label class="flex flex-col gap-1"><span class="text-muted">Username <span class="text-faint">(optional)</span></span><input bind:value={rules.upstream.username} autocomplete="off" class={field} /></label>
      <label class="flex flex-col gap-1"><span class="text-muted">Password</span><input type="password" bind:value={rules.upstream.password} autocomplete="off" class={field} /></label>
    </div>
    <label class="flex flex-col gap-1 text-[12.5px]"><span class="text-muted">Bypass <span class="text-faint">(one host per line, wildcards allowed)</span></span>
      <textarea
        value={bypassText}
        oninput={(e) => (rules.upstream.bypass = e.currentTarget.value.split("\n").map((l) => l.trim()).filter(Boolean))}
        rows="3"
        placeholder={"localhost\n*.internal.example.com"}
        class="mono rounded-lg border border-line bg-panel p-2 text-[12px] outline-none placeholder:text-faint focus:border-accent"
      ></textarea>
    </label>
    <p class="text-[11.5px] text-faint">The password is stored in <span class="mono">rules.json</span> in your ProxyZip data folder.</p>
  </TabFrame>

  <TabFrame
    title="SOCKS5 listener"
    description="Accept SOCKS5 clients too. Their traffic is decrypted, rewritten and recorded exactly like proxy traffic. It listens on the same interface as the main proxy."
    bind:enabled={rules.socks.enabled}
  >
    <label class="flex items-center gap-2 text-[12.5px]"><span class="text-muted">Port</span><input type="number" min="1" max="65535" bind:value={rules.socks.port} class="{field} w-28" /></label>
  </TabFrame>

  <TabFrame
    title="Reverse proxy"
    description="Expose a server on a local port. Point your app at localhost and ProxyZip forwards to the real target while recording everything."
    bind:enabled={rules.tools.reverse}
    onadd={() => rules.reverse.push({ name: "", enabled: true, listen_port: 8081, target: "https://" })}
    addLabel="Add reverse proxy"
    count={rules.reverse.length}
    empty="No reverse proxies yet."
  >
    {#each rules.reverse as rule, i (i)}
      <RuleCard bind:name={rule.name} bind:enabled={rule.enabled} ondelete={() => rules.reverse.splice(i, 1)} placeholder="Reverse proxy">
        <div class="flex items-center gap-2 text-[12.5px]">
          <span class="text-muted">localhost:</span>
          <input type="number" min="1" max="65535" bind:value={rule.listen_port} aria-label="Local port" class="{field} w-24" />
          <Icon name="chevronRight" size={14} class="text-faint" />
          <input bind:value={rule.target} spellcheck="false" placeholder="https://api.example.com/v1" aria-label="Target URL" class="{field} flex-1" />
        </div>
      </RuleCard>
    {/each}
  </TabFrame>
</div>
