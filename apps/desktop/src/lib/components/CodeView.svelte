<script lang="ts">
  import type { EditorLanguage } from "../editor";
  import CodeEditor from "./CodeEditor.svelte";

  // Read-only text with highlighting, search (⌘F), folding and line numbers.
  let { text, language = "text", wrap = false }: { text: string; language?: EditorLanguage; wrap?: boolean } = $props();
</script>

<CodeEditor value={text} {language} {wrap} readOnly />
