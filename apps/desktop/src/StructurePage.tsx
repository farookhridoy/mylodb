import { useEffect, useMemo, useState } from "react";
import Icon from "./Icon";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import { adminApi, api, describeError, serverApi, type Maintenance, type MaintenanceRow, type PlanOp, type TableRow } from "./api";
import { formatBytes } from "./browserMeta";
import FormDialog, { type FormField } from "./FormDialog";

/**
 * A database's Structure page (phpMyAdmin): every table and view with rows, engine, collation, size and overhead,
 * a "containing the word" filter, row actions, and a "With selected" menu. Everything that changes data goes through
 * `onPlan`, which shows the exact SQL first.
 */
export default function StructurePage(props: {
  connectionId: number;
  db: string;
  readOnly: boolean;
  refreshKey: number;
  onBrowse(table: string): void;
  onDesign(table: string): void;
  onNewTable(name: string, columns: number): void;
  onPlan(op: PlanOp, o: { title: string; summary?: string; confirmLabel?: string; after?: () => void }): void;
  onExport(tables: string[]): void;
  onSearch(table: string): void;
}) {
  const { db } = props;
  const [rows, setRows] = useState<TableRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [word, setWord] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [form, setForm] = useState<null | { title: string; fields: FormField[]; option?: { label: string; initial: boolean }; hint?: string; make(v: Record<string, string>, option: boolean): { op: PlanOp; title: string } | null }>(null);
  const [report, setReport] = useState<{ title: string; text?: string; rows?: MaintenanceRow[] } | null>(null);
  const [newName, setNewName] = useState("");
  const [newCols, setNewCols] = useState("4");

  useEffect(() => {
    let live = true;
    setRows(null);
    adminApi.tables(props.connectionId, db).then((r) => { if (live) { setRows(r); setError(null); setChecked(new Set()); } }).catch((e) => live && setError(describeError(e)));
    return () => { live = false; };
  }, [props.connectionId, db, props.refreshKey]);

  const shown = useMemo(() => {
    const w = word.trim().toLowerCase();
    return (rows ?? []).filter((r) => !w || r.name.toLowerCase().includes(w));
  }, [rows, word]);
  const picked = shown.filter((r) => checked.has(r.name));
  const names = picked.map((r) => r.name);
  const toggle = (n: string) => setChecked((c) => { const x = new Set(c); x.has(n) ? x.delete(n) : x.add(n); return x; });
  const allChecked = shown.length > 0 && shown.every((r) => checked.has(r.name));
  const none = picked.length === 0;
  const objects = picked.map((r) => [r.name, r.is_view] as [string, boolean]);
  const tablesOnly = picked.filter((r) => !r.is_view).map((r) => r.name);

  const maintain = async (op: Maintenance, label: string) => {
    try {
      const out = await serverApi.maintain(props.connectionId, db, tablesOnly, op);
      setReport({ title: `${label} ${tablesOnly.length} table${tablesOnly.length === 1 ? "" : "s"}`, rows: out });
    } catch (e) { setReport({ title: label, text: describeError(e) }); }
  };
  const showCreate = async () => {
    try {
      const parts = await Promise.all(picked.map((r) => api.objectDefinition(props.connectionId, r.is_view ? "view" : "table", db, r.name)));
      setReport({ title: "Show create", text: parts.join(";\n\n") + ";" });
    } catch (e) { setReport({ title: "Show create", text: describeError(e) }); }
  };
  const ask = (f: NonNullable<typeof form>) => setForm(f);

  const withSelected = (): MenuItem[] => [
    { label: "Copy table…", icon: "copy", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => ask({ title: "Copy tables", hint: "Each table is copied inside this database under the new name.",
        fields: tablesOnly.map((n) => ({ key: n, label: `Copy of ${n}`, value: `${n}_copy` })), option: { label: "Copy the rows too", initial: true },
        make: (v, d) => ({ title: "Copy tables", op: { op: "copy_tables", db, copies: tablesOnly.map((n) => [n, v[n]] as [string, string]), with_data: d } }) }) },
    { label: "Show create", icon: "sql", disabled: none, onClick: showCreate },
    { label: "Export…", icon: "export", disabled: none, onClick: () => props.onExport(names) },
    { separator: true },
    { label: "Empty…", icon: "eraser", danger: true, disabled: props.readOnly || tablesOnly.length === 0, onClick: () => props.onPlan({ op: "empty_tables", db, objects: tablesOnly.map((n) => [n, false] as [string, boolean]) }, { title: `Empty ${tablesOnly.length} table(s)`, confirmLabel: "Empty", summary: "All rows of the selected tables will be deleted and cannot be restored." }) },
    { label: "Drop…", icon: "trash", danger: true, disabled: props.readOnly || none, onClick: () => props.onPlan({ op: "drop_tables", db, objects }, { title: `Drop ${picked.length} object(s)`, confirmLabel: "Drop", summary: `${picked.length} selected table(s) or view(s) will be permanently deleted.` }) },
    { separator: true },
    { label: "Analyze table", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => maintain({ op: "analyze" }, "Analyze") },
    { label: "Check table", disabled: tablesOnly.length === 0, onClick: () => maintain({ op: "check", option: null }, "Check") },
    { label: "Checksum table", disabled: tablesOnly.length === 0, onClick: () => props.onPlan({ op: "checksum", db, tables: tablesOnly }, { title: "Checksum", confirmLabel: "Run" }) },
    { label: "Optimize table", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => maintain({ op: "optimize" }, "Optimize") },
    { label: "Repair table", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => maintain({ op: "repair", quick: false, extended: false, use_frm: false }, "Repair") },
    { separator: true },
    { label: "Add prefix to table…", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => ask({ title: "Add prefix", fields: [{ key: "prefix", label: "Prefix", value: "" }],
        make: (v) => ({ title: "Add prefix", op: { op: "add_prefix", db, tables: tablesOnly, prefix: v.prefix } }) }) },
    { label: "Replace table prefix…", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => ask({ title: "Replace table prefix", fields: [{ key: "old", label: "From", value: "" }, { key: "new", label: "To", value: "" }],
        make: (v) => ({ title: "Replace prefix", op: { op: "replace_prefix", db, tables: tablesOnly, old: v.old, new: v.new } }) }) },
    { label: "Copy table with prefix…", disabled: props.readOnly || tablesOnly.length === 0, onClick: () => ask({ title: "Copy table with prefix", fields: [{ key: "prefix", label: "Prefix", value: "" }], option: { label: "Copy the rows too", initial: true },
        make: (v, d) => ({ title: "Copy with prefix", op: { op: "copy_with_prefix", db, tables: tablesOnly, prefix: v.prefix, with_data: d } }) }) },
  ];

  const total = shown.reduce((a, r) => a + (r.bytes ?? 0), 0);
  const totalRows = shown.reduce((a, r) => a + (r.rows ?? 0), 0);
  const overhead = shown.reduce((a, r) => a + (r.overhead ?? 0), 0);

  return (
    <div className="admin-page">
      <h2><Icon name="database" size={16} /> {db} <span className="dim small">Structure</span></h2>
      <div className="admin-bar">
        <input className="admin-search" placeholder="containing the word…" value={word} onChange={(e) => setWord(e.target.value)} aria-label="Filter tables" />
        <span className="dim small">{rows ? `${shown.length} of ${rows.length}` : ""}</span>
        <span className="grow" />
        <button disabled={none} aria-label="With selected" onClick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); setMenu({ x: r.left, y: r.bottom + 2 }); }}>
          With selected{picked.length ? ` (${picked.length})` : ""} ▾
        </button>
      </div>
      {error && <div className="banner bad">{error}</div>}
      {!rows && !error && <div className="empty">Loading…</div>}
      {rows && (
        <table className="info admin-table structure" aria-label="Tables">
          <thead><tr>
            <th style={{ width: 26 }}><input type="checkbox" aria-label="Check all" checked={allChecked} onChange={(e) => setChecked(e.target.checked ? new Set(shown.map((r) => r.name)) : new Set())} /></th>
            <th>Table</th><th>Action</th><th className="num">Rows</th><th>Engine</th><th>Collation</th><th className="num">Size</th><th className="num">Overhead</th>
          </tr></thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.name} className={checked.has(r.name) ? "picked" : ""}>
                <td><input type="checkbox" checked={checked.has(r.name)} onChange={() => toggle(r.name)} aria-label={`Select ${r.name}`} /></td>
                <td><button className="link" onClick={() => props.onBrowse(r.name)}><Icon name={r.is_view ? "view" : "table"} size={13} /> {r.name}</button>{r.comment && <span className="dim small" title={r.comment}> ⓘ</span>}</td>
                <td className="row-actions">
                  <button className="link" onClick={() => props.onBrowse(r.name)}>Browse</button>
                  <button className="link" onClick={() => props.onDesign(r.name)}>Structure</button>
                  <button className="link" onClick={() => props.onSearch(r.name)}>Search</button>
                  <button className="link" disabled={props.readOnly || r.is_view} onClick={() => props.onBrowse(r.name)}>Insert</button>
                  <button className="link danger" disabled={props.readOnly || r.is_view} onClick={() => props.onPlan({ op: "empty_tables", db, objects: [[r.name, false]] }, { title: `Empty ${r.name}`, confirmLabel: "Empty", summary: `All rows of ${db}.${r.name} will be deleted.` })}>Empty</button>
                  <button className="link danger" disabled={props.readOnly} onClick={() => props.onPlan({ op: "drop_tables", db, objects: [[r.name, r.is_view]] }, { title: `Drop ${r.name}`, confirmLabel: "Drop", summary: `${db}.${r.name} will be permanently deleted.` })}>Drop</button>
                </td>
                <td className="num" title="Estimate from the server's statistics">{r.is_view ? "" : `≈ ${(r.rows ?? 0).toLocaleString()}`}</td>
                <td>{r.is_view ? <span className="dim">View</span> : r.engine}</td>
                <td className="mono">{r.collation ?? ""}</td>
                <td className="num">{r.bytes == null ? "" : formatBytes(r.bytes)}</td>
                <td className="num">{r.overhead ? formatBytes(r.overhead) : ""}</td>
              </tr>
            ))}
            {shown.length === 0 && <tr><td colSpan={8} className="empty small">No tables{word ? " match" : ""}</td></tr>}
          </tbody>
          <tfoot><tr><td /><td><b>{shown.length} tables</b></td><td /><td className="num"><b>≈ {totalRows.toLocaleString()}</b></td><td /><td /><td className="num"><b>{formatBytes(total)}</b></td><td className="num"><b>{overhead ? formatBytes(overhead) : ""}</b></td></tr></tfoot>
        </table>
      )}

      <section className="admin-card" style={{ marginTop: 14, maxWidth: 560 }}>
        <h4>Create new table</h4>
        <div className="admin-row">
          <input placeholder="Table name" value={newName} onChange={(e) => setNewName(e.target.value)} disabled={props.readOnly} aria-label="New table name" />
          <input type="number" min={1} max={200} style={{ width: 70 }} value={newCols} onChange={(e) => setNewCols(e.target.value)} disabled={props.readOnly} aria-label="Number of columns" title="Number of columns" />
          <button className="primary" disabled={props.readOnly || !newName.trim()} onClick={() => props.onNewTable(newName.trim(), Math.max(1, parseInt(newCols, 10) || 1))}>Create</button>
        </div>
      </section>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={withSelected()} onClose={() => setMenu(null)} />}
      {form && <FormDialog title={form.title} fields={form.fields} option={form.option} hint={form.hint} onCancel={() => setForm(null)}
        onSubmit={(v, d) => { const f = form; setForm(null); const made = f.make(v, d); if (made) props.onPlan(made.op, { title: made.title }); }} />}
      {report && (
        <div className="overlay"><div className="dialog" role="dialog" aria-label={report.title} style={{ minWidth: 560, maxWidth: 820 }}>
          <h3>{report.title}</h3>
          {report.text !== undefined && <pre className="preview-sql" style={{ maxHeight: 360 }}>{report.text}</pre>}
          {report.rows && <table className="info"><thead><tr><th>Table</th><th>Operation</th><th>Type</th><th>Message</th></tr></thead>
            <tbody>{report.rows.map((r, i) => <tr key={i}><td>{r.table}</td><td>{r.op}</td><td>{r.msg_type}</td><td>{r.msg_text}</td></tr>)}</tbody></table>}
          <div className="actions">
            {report.text !== undefined && <button onClick={() => navigator.clipboard?.writeText(report.text!).catch(() => {})}><Icon name="copy" /> Copy</button>}
            <span className="grow" /><button className="primary" onClick={() => setReport(null)}>Close</button>
          </div>
        </div></div>
      )}
    </div>
  );
}
