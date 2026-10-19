<script lang="ts">
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { bracketMatching, foldGutter, foldKeymap, indentOnInput } from "@codemirror/language";
  import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
  import { Compartment, EditorState } from "@codemirror/state";
  import { EditorView, drawSelection, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers, placeholder as placeholderExt } from "@codemirror/view";
  import { onDestroy, onMount } from "svelte";
  import { HIGHLIGHT_LIMIT, editorTheme, languageExtension, type EditorLanguage } from "../editor";

  let {
    value,
    language = "text",
    readOnly = true,
    wrap = false,
    gutter = true,
    placeholder = "",
    onchange,
  }: {
    value: string;
    language?: EditorLanguage;
    readOnly?: boolean;
    wrap?: boolean;
    gutter?: boolean;
    placeholder?: string;
    onchange?: (value: string) => void;
  } = $props();

  let host: HTMLDivElement | undefined = $state();
  let view: EditorView | undefined;
  const languageSlot = new Compartment();
  const wrapSlot = new Compartment();
  const readOnlySlot = new Compartment();
  const placeholderSlot = new Compartment();

  const lang = () => (value.length > HIGHLIGHT_LIMIT ? [] : (languageExtension(language) ?? []));

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          gutter ? [lineNumbers(), highlightActiveLineGutter(), foldGutter()] : [],
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          keymap.of([...defaultKeymap, ...historyKeymap, ...foldKeymap, ...searchKeymap, indentWithTab]),
          editorTheme,
          languageSlot.of(lang()),
          wrapSlot.of(wrap ? EditorView.lineWrapping : []),
          readOnlySlot.of(EditorState.readOnly.of(readOnly)),
          placeholderSlot.of(placeholder ? placeholderExt(placeholder) : []),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && onchange) onchange(update.state.doc.toString());
          }),
        ],
      }),
    });
  });

  onDestroy(() => view?.destroy());

  // Keep the editor in step with the props.
  $effect(() => {
    const next = value;
    if (view && next !== view.state.doc.toString()) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
    }
  });
  $effect(() => {
    const l = language;
    void l;
    view?.dispatch({ effects: languageSlot.reconfigure(lang()) });
  });
  $effect(() => {
    view?.dispatch({ effects: wrapSlot.reconfigure(wrap ? EditorView.lineWrapping : []) });
  });
  $effect(() => {
    view?.dispatch({ effects: readOnlySlot.reconfigure(EditorState.readOnly.of(readOnly)) });
  });
</script>

<div bind:this={host} class="selectable h-full min-h-0 w-full overflow-hidden"></div>
