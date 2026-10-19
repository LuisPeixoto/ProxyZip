export function methodClass(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "text-info";
    case "POST":
      return "text-ok";
    case "PUT":
      return "text-[#f97316]";
    case "PATCH":
      return "text-warn";
    case "DELETE":
      return "text-err";
    case "CONNECT":
      return "text-accent";
    default:
      return "text-muted";
  }
}

export function statusClass(status: number | null, failed: boolean): string {
  if (failed) return "bg-err/15 text-err";
  if (status == null) return "bg-faint/15 text-faint";
  if (status >= 500) return "bg-err/15 text-err";
  if (status >= 400) return "bg-warn/15 text-warn";
  if (status >= 300) return "bg-accent-2/15 text-accent-2";
  if (status >= 200) return "bg-ok/15 text-ok";
  return "bg-info/15 text-info";
}

/** Short human label for a MIME type. */
export function typeLabel(ct: string | null): string {
  if (!ct) return "";
  const c = ct.toLowerCase();
  if (c.includes("json")) return "JSON";
  if (c.includes("html")) return "HTML";
  if (c.includes("javascript")) return "JS";
  if (c.includes("css")) return "CSS";
  if (c.includes("xml")) return "XML";
  if (c.includes("protobuf") || c.includes("grpc")) return "Protobuf";
  if (c.includes("event-stream")) return "SSE";
  if (c.includes("form-urlencoded")) return "Form";
  if (c.startsWith("multipart/")) return "Multipart";
  if (c.startsWith("image/")) return c.slice(6).toUpperCase().replace("SVG+XML", "SVG");
  if (c.startsWith("font/") || c.includes("woff")) return "Font";
  if (c.startsWith("video/")) return "Video";
  if (c.startsWith("audio/")) return "Audio";
  if (c.startsWith("text/")) return "Text";
  return c.split("/").pop() ?? c;
}
