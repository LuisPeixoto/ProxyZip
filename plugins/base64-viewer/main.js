// Viewer: `decode(body, info)` returns { format, json } for the "Decoded" view.
// `plugin.settings` holds the values the user chose in Settings.
function decode(body, info) {
  const text = $.base64.decode((body.text || "").replace(/\s+/g, ""));
  if (plugin.settings.parse_json) {
    try {
      return { format: "base64 + json", json: JSON.parse(text) };
    } catch (_) {
      // Not JSON: fall through and show the text.
    }
  }
  return { format: "base64", json: { text, length: text.length } };
}
