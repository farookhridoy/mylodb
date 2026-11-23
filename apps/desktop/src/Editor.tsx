import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { basicSetup } from "codemirror";
import { Compartment, EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { editorApi, type SuggestionKind } from "./api";
import { MySQL, sql } from "@codemirror/lang-sql";
import { oneDark } from "@codemirror/theme-one-dark";
import { schemeExtension } from "./editorSchemes";
import type { EditorScheme } from "./prefs";

export interface EditorHandle {
  /** Selected text, or the whole document when nothing is selected. */
  runnableText(): string;
  /** The selection if there is one, else the statement the cursor is in. */
  analysisTarget(): Promise<string>;
  /** Replace the selection (or the whole document when nothing is selected). */
  replaceRunnable(text: string): void;
  insert(text: string): void;
  focus(): void;
}

const TYPE: Record<SuggestionKind, string> = {
  column: "property", table: "class", view: "interface", database: "namespace", alias: "variable", function: "function", keyword: "keyword",
};

/**
 * One CodeMirror view shared by all tabs; each tab keeps its own EditorState
 * (document, selection, undo history) in `states`.
 */
const Editor = forwardRef<EditorHandle, {
  tabId: string;
  initialDoc: string;
  onChange(doc: string): void;
  onRun(): void;
  onFormat(): void;
  /** Connection and database used to resolve names for autocomplete. */
  connectionId: number;
  database: string | null;
  /** Dark editor colours (the light theme uses the app's own colours). */
  dark: boolean;
  /** Syntax colour scheme chosen in Preferences. */
  scheme?: EditorScheme;
  /** The colour theme in effect (Monokai, GitHub Dark…), whose syntax colours the editor uses unless a scheme is chosen. */
  theme?: string | null;
}>(function Editor(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const states = useRef(new Map<string, EditorState>());
  const current = useRef(props.tabId);
  const cb = useRef(props);
  cb.current = props;
  // the colour theme sits in a compartment so it can change without recreating the editor or losing undo history
  const themeSlot = useRef(new Compartment());
  const themeFor = (dark: boolean) => [dark ? oneDark : [], Prec.high(schemeExtension(cb.current.scheme ?? "default", dark, cb.current.theme))];

  /** Ask the Rust engine for candidates. Ranked and capped there, so CodeMirror must not filter again. */
  const completionSource = async (ctx: CompletionContext): Promise<CompletionResult | null> => {
    const word = ctx.matchBefore(/[\w$`.]*/);
    if (!ctx.explicit && (!word || word.from === word.to)) return null;
    const doc = ctx.state.doc.toString();
    const res = await editorApi.complete(cb.current.connectionId, doc, ctx.pos, cb.current.database).catch(() => null);
    if (!res || res.items.length === 0 || ctx.aborted) return null;
    return {
      from: res.from,
      to: res.to,
      filter: false,
      options: res.items.map((i, n) => ({
        label: i.label, type: TYPE[i.kind], detail: i.detail ?? undefined, apply: i.insert, boost: -n,
      })),
    };
  };

  const makeState = (doc: string) =>
    EditorState.create({
      doc,
      extensions: [
        basicSetup,
        sql({ dialect: MySQL, upperCaseKeywords: true }),
        themeSlot.current.of(themeFor(cb.current.dark)),
        autocompletion({ override: [completionSource], icons: true }),
        // highest precedence: the default keymap binds Mod-Enter to "insert blank line"
        Prec.highest(keymap.of([
          { key: "Mod-Enter", run: () => (cb.current.onRun(), true) },
          { key: "Shift-Mod-f", run: () => (cb.current.onFormat(), true) },
        ])),
        EditorView.updateListener.of((u) => u.docChanged && cb.current.onChange(u.state.doc.toString())),
        EditorView.theme({ "&": { height: "100%" } }), // fonts come from the Preferences CSS variables
      ],
    });

  useEffect(() => {
    view.current = new EditorView({ state: makeState(props.initialDoc), parent: host.current! });
    return () => view.current?.destroy();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Switching tabs: stash the old tab's state, restore (or create) the new one.
  useEffect(() => {
    const v = view.current;
    if (!v || current.current === props.tabId) return;
    states.current.set(current.current, v.state);
    current.current = props.tabId;
    v.setState(states.current.get(props.tabId) ?? makeState(props.initialDoc));
    v.dispatch({ effects: themeSlot.current.reconfigure(themeFor(props.dark)) });
    v.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tabId]);

  useEffect(() => {
    view.current?.dispatch({ effects: themeSlot.current.reconfigure(themeFor(props.dark)) });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.dark, props.scheme, props.theme]);

  useImperativeHandle(ref, () => ({
    runnableText() {
      const v = view.current!;
      const sel = v.state.selection.main;
      return sel.empty ? v.state.doc.toString() : v.state.sliceDoc(sel.from, sel.to);
    },
    async analysisTarget() {
      const v = view.current!;
      const sel = v.state.selection.main;
      if (!sel.empty) return v.state.sliceDoc(sel.from, sel.to);
      const found = await editorApi.statementAt(v.state.doc.toString(), sel.head).catch(() => null);
      return found ?? v.state.doc.toString();
    },
    replaceRunnable(text) {
      const v = view.current!;
      const sel = v.state.selection.main;
      const range = sel.empty ? { from: 0, to: v.state.doc.length } : { from: sel.from, to: sel.to };
      v.dispatch({ changes: { ...range, insert: text }, selection: { anchor: range.from + text.length } });
      v.focus();
    },
    insert(text) {
      const v = view.current!;
      v.dispatch(v.state.replaceSelection(text));
      v.focus();
    },
    focus: () => view.current?.focus(),
  }));

  return <div className="editor" ref={host} />;
});

export default Editor;
