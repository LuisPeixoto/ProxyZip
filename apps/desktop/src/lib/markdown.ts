/** A deliberately small Markdown subset for plugin tabs: headings, lists, code blocks, bold and inline code. No HTML. */

export interface Inline {
  text: string;
  bold?: boolean;
  code?: boolean;
}

export type Block =
  | { kind: "heading"; level: number; inline: Inline[] }
  | { kind: "list"; items: Inline[][] }
  | { kind: "code"; text: string }
  | { kind: "paragraph"; inline: Inline[] };

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  for (const m of text.matchAll(pattern)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    const token = m[0];
    out.push(token.startsWith("`") ? { text: token.slice(1, -1), code: true } : { text: token.slice(2, -2), bold: true });
    last = m.index + token.length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

export function parseMarkdown(source: string): Block[] {
  const blocks: Block[] = [];
  const lines = source.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.trim().startsWith("```")) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) code.push(lines[i++]);
      i++;
      blocks.push({ kind: "code", text: code.join("\n") });
    } else if (/^#{1,4}\s/.test(line)) {
      const level = line.match(/^#+/)![0].length;
      blocks.push({ kind: "heading", level, inline: parseInline(line.replace(/^#+\s+/, "")) });
      i++;
    } else if (/^\s*[-*]\s+/.test(line)) {
      const items: Inline[][] = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) items.push(parseInline(lines[i++].replace(/^\s*[-*]\s+/, "")));
      blocks.push({ kind: "list", items });
    } else if (line.trim() === "") {
      i++;
    } else {
      const para: string[] = [];
      while (i < lines.length && lines[i].trim() !== "" && !/^(#{1,4}\s|\s*[-*]\s+|```)/.test(lines[i])) para.push(lines[i++]);
      blocks.push({ kind: "paragraph", inline: parseInline(para.join(" ")) });
    }
  }
  return blocks;
}
