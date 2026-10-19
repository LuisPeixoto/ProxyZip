import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { java } from "@codemirror/lang-java";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { php } from "@codemirror/lang-php";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { xml } from "@codemirror/lang-xml";
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language";
import { csharp, kotlin } from "@codemirror/legacy-modes/mode/clike";
import { go } from "@codemirror/legacy-modes/mode/go";
import { powerShell } from "@codemirror/legacy-modes/mode/powershell";
import { ruby } from "@codemirror/legacy-modes/mode/ruby";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { swift } from "@codemirror/legacy-modes/mode/swift";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

/** Languages the editor can highlight. Anything else is shown as plain text. */
export type EditorLanguage =
  | "text" | "json" | "xml" | "html" | "javascript" | "css"
  | "shell" | "powershell" | "python" | "go" | "rust" | "java" | "kotlin" | "swift" | "csharp" | "php" | "ruby";

/** Bodies above this size are shown without highlighting so scrolling stays smooth. */
export const HIGHLIGHT_LIMIT = 2_000_000;

export function languageExtension(language: EditorLanguage): Extension | undefined {
  switch (language) {
    case "json":
      return json();
    case "xml":
      return xml();
    case "html":
      return html();
    case "javascript":
      return javascript();
    case "css":
      return css();
    case "python":
      return python();
    case "rust":
      return rust();
    case "java":
      return java();
    case "php":
      return php();
    case "shell":
      return StreamLanguage.define(shell);
    case "powershell":
      return StreamLanguage.define(powerShell);
    case "go":
      return StreamLanguage.define(go);
    case "swift":
      return StreamLanguage.define(swift);
    case "ruby":
      return StreamLanguage.define(ruby);
    case "kotlin":
      return StreamLanguage.define(kotlin);
    case "csharp":
      return StreamLanguage.define(csharp);
    default:
      return undefined;
  }
}

/** Colours come from the CSS variables in `app.css`, so the editor follows the light and dark themes. */
const highlight = HighlightStyle.define([
  { tag: [t.propertyName, t.attributeName], color: "var(--tok-key)" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "var(--tok-str)" },
  { tag: [t.number, t.integer, t.float], color: "var(--tok-num)" },
  { tag: [t.bool, t.keyword, t.operatorKeyword, t.modifier, t.definitionKeyword, t.controlKeyword], color: "var(--tok-bool)" },
  { tag: [t.null, t.atom, t.self], color: "var(--tok-null)" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--tok-comment)", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.className, t.typeName, t.tagName], color: "var(--tok-fn)" },
  { tag: [t.punctuation, t.separator, t.bracket, t.operator], color: "var(--faint)" },
  { tag: t.link, textDecoration: "underline" },
  { tag: t.invalid, color: "var(--err)" },
]);

export const editorTheme: Extension = [
  EditorView.theme({
    "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent", fontSize: "12px" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "19px", overflow: "auto" },
    ".cm-content": { caretColor: "var(--accent)", padding: "6px 0" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
    "&.cm-focused": { outline: "none" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": { backgroundColor: "var(--select)" },
    ".cm-activeLine": { backgroundColor: "var(--hover)" },
    ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--faint)", border: "none", borderRight: "1px solid var(--line)" },
    ".cm-activeLineGutter": { backgroundColor: "var(--hover)", color: "var(--fg)" },
    ".cm-foldPlaceholder": { backgroundColor: "var(--hover)", border: "none", color: "var(--muted)" },
    ".cm-panels": { backgroundColor: "var(--panel-2)", color: "var(--fg)", borderColor: "var(--line)" },
    ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--line)" },
    ".cm-searchMatch": { backgroundColor: "var(--accent-soft)", outline: "1px solid var(--accent)" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--select)" },
    ".cm-textfield": { backgroundColor: "var(--panel)", border: "1px solid var(--line)", color: "var(--fg)", borderRadius: "6px", padding: "2px 6px" },
    ".cm-button": { backgroundImage: "none", backgroundColor: "var(--panel)", border: "1px solid var(--line)", color: "var(--fg)", borderRadius: "6px" },
    ".cm-tooltip": { backgroundColor: "var(--elevated)", border: "1px solid var(--line)", color: "var(--fg)" },
    // Side-by-side diff colours
    ".cm-changedLine": { backgroundColor: "var(--diff-add)" },
    ".cm-deletedChunk": { backgroundColor: "var(--diff-del)" },
    ".cm-insertedLine, .cm-changedText": { backgroundColor: "var(--diff-add)" },
    ".cm-deletedLine": { backgroundColor: "var(--diff-del)" },
  }),
  syntaxHighlighting(highlight),
];

/** Pick a highlighter from a `Content-Type` value. */
export function languageForMime(mime: string | null | undefined): EditorLanguage {
  const m = (mime ?? "").toLowerCase();
  if (m.includes("json")) return "json";
  if (m.includes("html")) return "html";
  if (m.includes("xml")) return "xml";
  if (m.includes("javascript") || m.includes("ecmascript")) return "javascript";
  if (m.includes("css")) return "css";
  return "text";
}
