<script lang="ts">
  // Lucide-style 24px stroke icons, inlined so the app works fully offline.
  const paths: Record<string, string> = {
    record: "M12 12m-6 0a6 6 0 1 0 12 0a6 6 0 1 0 -12 0",
    pause: "M8 5v14M16 5v14",
    trash: "M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v6M14 11v6",
    search: "M11 11m-7 0a7 7 0 1 0 14 0a7 7 0 1 0 -14 0M21 21l-4.3-4.3",
    globe: "M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0 -18 0M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18",
    lock: "M5 11h14v10H5zM8 11V7a4 4 0 0 1 8 0v4",
    unlock: "M5 11h14v10H5zM8 11V7a4 4 0 0 1 7.5-2",
    shield: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z",
    shieldCheck: "M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6zM9 12l2 2 4-4",
    settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    sun: "M12 12m-4 0a4 4 0 1 0 8 0a4 4 0 1 0 -8 0M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
    moon: "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
    download: "M12 3v12M7 10l5 5 5-5M5 21h14",
    upload: "M12 21V9M7 14l5-5 5 5M5 3h14",
    pin: "M12 17v5M9 3h6l-1 6 3 3v2H7v-2l3-3z",
    alert: "M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z",
    layers: "M12 2 2 7l10 5 10-5zM2 17l10 5 10-5M2 12l10 5 10-5",
    zap: "M13 2 3 14h9l-1 8 10-12h-9z",
    monitor: "M3 4h18v12H3zM8 20h8M12 16v4",
    laptop: "M4 5h16v11H4zM2 19h20",
    copy: "M9 9h11v11H9zM5 15H4V4h11v1",
    repeat: "M17 1l4 4-4 4M3 11V9a4 4 0 0 1 4-4h14M7 23l-4-4 4-4M21 13v2a4 4 0 0 1-4 4H3",
    x: "M18 6 6 18M6 6l12 12",
    check: "M20 6 9 17l-5-5",
    chevronRight: "M9 18l6-6-6-6",
    chevronDown: "M6 9l6 6 6-6",
    terminal: "M4 17l6-6-6-6M12 19h8",
    sidebar: "M3 4h18v16H3zM9 4v16",
    filter: "M22 3H2l8 9.5V19l4 2v-8.5z",
    message: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
    arrowUp: "M12 19V5M5 12l7-7 7 7",
    arrowDown: "M12 5v14M19 12l-7 7-7-7",
    smartphone: "M7 2h10v20H7zM12 18h.01",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    info: "M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0 -18 0M12 16v-4M12 8h.01",
    code: "M16 18l6-6-6-6M8 6l-6 6 6 6",
    clock: "M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0 -18 0M12 7v5l3 2",
    comment: "M4 4h16v12H8l-4 4z",
    power: "M12 2v10M18.4 6.6a9 9 0 1 1-12.8 0",
    wrench: "M14.7 6.3a4 4 0 0 0 5 5L21 13l-8 8a2.8 2.8 0 0 1-4-4l8-8zM14.7 6.3 18 3l3 3-3.3 3.3",
    send: "M22 2 11 13M22 2l-7 20-4-9-9-4z",
    plus: "M12 5v14M5 12h14",
    ban: "M12 12m-9 0a9 9 0 1 0 18 0a9 9 0 1 0 -18 0M5.6 5.6l12.8 12.8",
    edit: "M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z",
    play: "M6 4l14 8-14 8z",
    sliders: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
    shuffle: "M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5",
    gauge: "M12 14l4-4M3.3 17a10 10 0 1 1 17.4 0",
    network: "M12 3v6M5 21v-6a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v6M9 3h6v6H9zM2 21h6v0M16 21h6",
    eye: "M2 12s4-8 10-8 10 8 10 8-4 8-10 8-10-8-10-8zM12 12m-3 0a3 3 0 1 0 6 0a3 3 0 1 0 -6 0",
    file: "M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM14 3v6h6",
  };
  let { name, size = 16, class: cls = "", stroke = 1.8 }: { name: string; size?: number; class?: string; stroke?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  class={cls}
  aria-hidden="true"><path d={paths[name] ?? paths.info} /></svg>
