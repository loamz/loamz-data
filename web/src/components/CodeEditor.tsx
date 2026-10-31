import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { python } from "@codemirror/lang-python";
import { PostgreSQL, sql } from "@codemirror/lang-sql";
import { HighlightStyle, bracketMatching, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView, drawSelection, highlightActiveLine, keymap, lineNumbers, placeholder } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { useEffect, useRef } from "react";

/** The editor looks like the rest of the app: every colour is one of its tokens, so light and dark follow by themselves. */
const look = EditorView.theme({
  "&": { color: "var(--foreground)", backgroundColor: "transparent", fontSize: "13px", height: "100%" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-geist-mono)", lineHeight: "1.6", overflow: "auto" },
  ".cm-content": { caretColor: "var(--accent)", padding: "8px 0" },
  ".cm-cursor": { borderLeftColor: "var(--accent)" },
  ".cm-gutters": { backgroundColor: "transparent", color: "var(--muted-foreground)", border: "none" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--accent) 6%, transparent)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--foreground)" },
  ".cm-selectionBackground, &.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": { backgroundColor: "color-mix(in srgb, var(--accent) 25%, transparent)" },
  ".cm-placeholder": { color: "var(--muted-foreground)" },
  ".cm-tooltip": { backgroundColor: "var(--card)", color: "var(--foreground)", border: "1px solid var(--border)", borderRadius: "8px", boxShadow: "var(--shadow-md)" },
  ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: "color-mix(in srgb, var(--accent) 20%, transparent)", color: "var(--foreground)" },
  ".cm-matchingBracket": { backgroundColor: "color-mix(in srgb, var(--accent) 22%, transparent)" },
});

const colours = HighlightStyle.define([
  { tag: [t.keyword, t.operatorKeyword, t.modifier], color: "var(--accent)", fontWeight: "600" },
  { tag: [t.string, t.special(t.string)], color: "var(--success)" },
  { tag: [t.number, t.bool, t.null], color: "var(--warning)" },
  { tag: [t.lineComment, t.blockComment], color: "var(--muted-foreground)", fontStyle: "italic" },
  { tag: [t.typeName, t.className, t.definition(t.variableName), t.function(t.variableName)], color: "var(--foreground)" },
  { tag: t.operator, color: "var(--muted)" },
]);

export interface CodeEditorHandle {
  /** What is selected, or null when nothing is. */
  selection: () => string | null;
}

/**
 * A code editor (CodeMirror 6). `value` is kept in step both ways: typing calls `onChange`, and a new `value` from outside (another tab)
 * replaces the text. Ctrl or Cmd + Enter calls `onRun` with the selection, if there is one.
 */
export function CodeEditor({ value, onChange, onRun, language, names = [], hint, handle }: { value: string; onChange: (v: string) => void; onRun?: (selection: string | null) => void; language: "sql" | "python"; names?: string[]; hint?: string; handle?: React.MutableRefObject<CodeEditorHandle | null> }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latest = useRef({ onChange, onRun });
  latest.current = { onChange, onRun };

  useEffect(() => {
    if (!host.current) return;
    // Table names for completion, written the way SQL here writes them: catalog.schema.table.
    const schema = Object.fromEntries(names.map((n) => [n, [] as string[]]));
    const run = () => {
      const v = view.current;
      if (!v) return true;
      const { from, to } = v.state.selection.main;
      latest.current.onRun?.(from === to ? null : v.state.sliceDoc(from, to));
      return true;
    };
    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        history(),
        drawSelection(),
        highlightActiveLine(),
        indentOnInput(),
        bracketMatching(),
        syntaxHighlighting(colours),
        language === "sql" ? sql({ dialect: PostgreSQL, schema, upperCaseKeywords: true }) : python(),
        placeholder(hint ?? ""),
        keymap.of([{ key: "Mod-Enter", run }, indentWithTab, ...defaultKeymap, ...historyKeymap]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) latest.current.onChange(u.state.doc.toString());
        }),
        look,
      ],
    });
    const v = new EditorView({ state, parent: host.current });
    view.current = v;
    if (handle) handle.current = { selection: () => (v.state.selection.main.empty ? null : v.state.sliceDoc(v.state.selection.main.from, v.state.selection.main.to)) };
    return () => {
      v.destroy();
      view.current = null;
    };
    // The editor is made once for a language and a set of names; the text is kept in step below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [language, names.join("\n")]);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== value) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
  }, [value]);

  return <div ref={host} className="h-full min-h-0 w-full overflow-hidden" />;
}
