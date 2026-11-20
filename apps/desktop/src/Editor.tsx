import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { MySQL, sql } from "@codemirror/lang-sql";
import { oneDark } from "@codemirror/theme-one-dark";

export interface EditorHandle {
  /** Selected text, or the whole document when nothing is selected. */
  runnableText(): string;
  insert(text: string): void;
  focus(): void;
}

/**
 * One CodeMirror view shared by all tabs; each tab keeps its own EditorState
 * (document, selection, undo history) in `states`.
 */
const Editor = forwardRef<EditorHandle, {
  tabId: string;
  initialDoc: string;
  onChange(doc: string): void;
  onRun(): void;
}>(function Editor(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const states = useRef(new Map<string, EditorState>());
  const current = useRef(props.tabId);
  const cb = useRef(props);
  cb.current = props;

  const makeState = (doc: string) =>
    EditorState.create({
      doc,
      extensions: [
        basicSetup,
        sql({ dialect: MySQL, upperCaseKeywords: true }),
        oneDark,
        keymap.of([{ key: "Mod-Enter", run: () => (cb.current.onRun(), true) }]),
        EditorView.updateListener.of((u) => u.docChanged && cb.current.onChange(u.state.doc.toString())),
        EditorView.theme({ "&": { height: "100%" }, ".cm-scroller": { fontFamily: "SF Mono, Menlo, monospace", fontSize: "13px" } }),
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
    v.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.tabId]);

  useImperativeHandle(ref, () => ({
    runnableText() {
      const v = view.current!;
      const sel = v.state.selection.main;
      return sel.empty ? v.state.doc.toString() : v.state.sliceDoc(sel.from, sel.to);
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
