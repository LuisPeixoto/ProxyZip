<script lang="ts">
  import { app } from "../stores/app.svelte";
  import { api, base64ToText, errorText, textToBase64, type ComposeInput } from "../api";
  import { languageForMime } from "../editor";
  import CodeEditor from "./CodeEditor.svelte";
  import HeadersEditor from "./HeadersEditor.svelte";
  import Icon from "./Icon.svelte";

  const methods = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
  const start = app.composer as ComposeInput;

  let method = $state(start.method);
  let url = $state(start.url);
  let headers = $state<[string, string][]>(start.headers.map(([k, v]) => [k, v] as [string, string]));
  let body = $state(base64ToText(start.body_base64) ?? "");
  let binary = $state(start.body_base64 !== "" && base64ToText(start.body_base64) === null);
  let importing = $state(false);
  let curl = $state("");
  let sending = $state(false);
  let lastStatus = $state<number | null>(null);
  const bodyLanguage = $derived(languageForMime(headers.find(([k]) => k.toLowerCase() === "content-type")?.[1]));

  async function importCurl() {
    try {
      const parsed = await api.parseCurl(curl);
      method = parsed.method;
      url = parsed.url;
      headers = parsed.headers.map(([k, v]) => [k, v] as [string, string]);
      body = base64ToText(parsed.body_base64) ?? "";
      binary = false;
      importing = false;
      curl = "";
    } catch (e) {
      app.toast(errorText(e), "err");
    }
  }

  async function send() {
    sending = true;
    try {
      lastStatus = await api.sendRequest({
        method,
        url,
        headers: headers.filter(([k]) => k.trim() !== ""),
        body_base64: binary ? start.body_base64 : textToBase64(body),
      });
      app.toast(`Sent: ${lastStatus}`, lastStatus >= 400 ? "err" : "ok");
    } catch (e) {
      lastStatus = null;
      app.toast(errorText(e), "err");
    } finally {
      sending = false;
    }
  }

  function close() {
    app.composer = null;
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") close();
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") send();
  }

  const field = "mono h-8 rounded-lg border border-line bg-panel px-2.5 text-[12px] outline-none focus:border-accent";
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-40 grid place-items-center bg-black/40 p-6 backdrop-blur-[2px]" onclick={close}>
  <div class="shadow-pop flex max-h-full w-full max-w-[760px] flex-col overflow-hidden rounded-2xl border border-line bg-elevated" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true" aria-label="Compose request" tabindex="-1">
    <header class="flex items-center gap-3 border-b border-line px-5 py-3">
      <span class="grid size-8 place-items-center rounded-lg bg-accent-soft text-accent"><Icon name="send" size={16} /></span>
      <h2 class="flex-1 text-[14px] font-semibold">Compose request</h2>
      <button onclick={() => (importing = !importing)} class="flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[12px] text-muted hover:bg-hover hover:text-fg"><Icon name="terminal" size={13} /> Import cURL</button>
      <button onclick={close} class="grid size-7 place-items-center rounded-md text-muted hover:bg-hover hover:text-fg" aria-label="Close"><Icon name="x" /></button>
    </header>

    <div class="selectable min-h-0 flex-1 space-y-4 overflow-y-auto p-5 text-[12.5px]">
      {#if importing}
        <div class="space-y-2 rounded-xl border border-line bg-panel-2 p-3">
          <textarea bind:value={curl} rows="4" spellcheck="false" placeholder={"Paste a cURL command, for example one copied from browser dev tools:\ncurl 'https://api.example.com/items' -H 'accept: application/json'"} class="mono w-full resize-y rounded-lg border border-line bg-panel p-2.5 text-[12px] outline-none placeholder:text-faint focus:border-accent"></textarea>
          <div class="flex justify-end"><button onclick={importCurl} disabled={!curl.trim()} class="rounded-lg bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90 disabled:opacity-50">Fill the form</button></div>
        </div>
      {/if}
      <div class="flex gap-2">
        <select bind:value={method} aria-label="Method" class="{field} w-28 font-semibold">
          {#each methods as m (m)}<option>{m}</option>{/each}
          {#if !methods.includes(method)}<option>{method}</option>{/if}
        </select>
        <input bind:value={url} aria-label="URL" spellcheck="false" placeholder="https://api.example.com/v1/items" class="{field} min-w-0 flex-1" />
      </div>
      <div>
        <div class="mb-1.5 text-[11.5px] font-semibold tracking-wide text-muted uppercase">Headers</div>
        <HeadersEditor bind:headers />
      </div>
      <div>
        <div class="mb-1.5 text-[11.5px] font-semibold tracking-wide text-muted uppercase">Body</div>
        {#if binary}
          <p class="rounded-lg border border-dashed border-line px-3 py-4 text-center text-faint">The original body is binary and will be sent unchanged.</p>
        {:else}
          <div class="h-[240px] overflow-hidden rounded-lg border border-line bg-panel focus-within:border-accent">
            <CodeEditor value={body} onchange={(v) => (body = v)} readOnly={false} language={bodyLanguage} placeholder="No body" />
          </div>
        {/if}
      </div>
    </div>

    <footer class="flex items-center justify-between gap-2 border-t border-line px-5 py-3">
      <span class="text-[11.5px] text-faint">The request goes through ProxyZip, so it appears in the list and your rules apply. {#if lastStatus !== null}Last response: <span class="mono text-fg">{lastStatus}</span>{/if}</span>
      <button onclick={send} disabled={sending || !url.trim()} class="flex items-center gap-1.5 rounded-lg bg-accent px-4 py-1.5 text-[12.5px] font-medium text-white hover:opacity-90 disabled:opacity-50">
        <Icon name="send" size={13} /> {sending ? "Sending…" : "Send"}
      </button>
    </footer>
  </div>
</div>
