import { useEffect, useMemo, useState } from "react";
import { plainError, usagesApi, type Usage } from "./api";

/** "Where is this used?": the views, routines, triggers, events and keys that mention a table, optionally one of its columns. */
export default function UsagesDialog(props: { connectionId: number; db: string; table: string; columns: string[]; onClose(): void }) {
  const [column, setColumn] = useState("");
  const [rows, setRows] = useState<Usage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setRows(null); setError(null);
    usagesApi.find(props.connectionId, props.db, props.table, column || null).then((r) => live && setRows(r)).catch((e) => live && setError(plainError(e)));
    return () => { live = false; };
  }, [props.connectionId, props.db, props.table, column]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props]);
  const groups = useMemo(() => {
    const m = new Map<string, Usage[]>();
    for (const u of rows ?? []) m.set(u.kind, [...(m.get(u.kind) ?? []), u]);
    return [...m.entries()];
  }, [rows]);
  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog usages-dialog" role="dialog" aria-label="Where is this used" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Where is <code>{props.table}{column ? `.${column}` : ""}</code> used?</h3>
        <label className="usage-col">Narrow to a column
          <select value={column} onChange={(e) => setColumn(e.target.value)} aria-label="Column"><option value="">(the whole table)</option>{props.columns.map((c) => <option key={c} value={c}>{c}</option>)}</select></label>
        {error && <div className="banner bad" role="alert">{error}</div>}
        {rows === null && !error && <div className="empty">Searching…</div>}
        {rows?.length === 0 && <div className="empty">Nothing else uses {column ? "this column" : "this table"}.</div>}
        <div className="usage-list">
          {groups.map(([kind, items]) => (
            <section key={kind}>
              <h4>{kind} <span className="dim">({items.length})</span></h4>
              <ul>{items.map((u, i) => <li key={i}><b>{u.db === props.db ? u.name : `${u.db}.${u.name}`}</b><code className="dim small">{u.detail}</code></li>)}</ul>
            </section>
          ))}
        </div>
        <p className="dim small">Renaming a table or column does not change these. Edit each one yourself, or rename and fix them straight after.</p>
        <div className="actions"><button className="primary" onClick={props.onClose}>Close</button></div>
      </div>
    </div>
  );
}
