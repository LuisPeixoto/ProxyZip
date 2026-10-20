// Tab: `render(flow, info)` returns { type, content } shown in the inspector.
// Types: "table" ({ columns, rows }), "json", "text" and "markdown".
function render(flow, info) {
  const t = flow.timing;
  if (!t || t.finished == null) {
    return { type: "text", content: "The request has not finished yet." };
  }
  const sent = t.request_sent ?? t.started;
  const first = t.response_started ?? t.finished;
  const rows = [
    ["Sending the request", sent - t.started],
    ["Waiting for the first byte", first - sent],
    ["Downloading the response", t.finished - first],
    ["Total", t.finished - t.started],
  ];
  return {
    type: "table",
    content: { columns: ["Phase", "Time (ms)"], rows: rows.map(([phase, ms]) => [phase, String(ms)]) },
  };
}
