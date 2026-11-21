import { useEffect, useMemo, useState } from "react";
import Icon from "./Icon";
import Select from "./Select";
import { adminApi, describeError, schemaApi, type DatabaseRow } from "./api";
import { formatBytes } from "./browserMeta";

type SortKey = "name" | "collation" | "tables" | "size";

/**
 * The server's Databases page (phpMyAdmin's "Databases" tab): every database with its collation, table count and size,
 * Check all, and a Drop that opens the Preview SQL dialog. Names come first; sizes follow in a second query because that one
 * reads the statistics of every table on the server.
 */
export default function DatabasesPage(props: {
  connectionId: number;
  readOnly: boolean;
  /** Refresh counter: the page reloads when it changes (after a drop or create). */
  refreshKey: number;
  onOpenDatabase(db: string): void;
  onCreate(name: string, collation: string | null): void;
  onDrop(names: string[], summary: string): void;
}) {
  const [rows, setRows] = useState<DatabaseRow[] | null>(null);
  const [sizes, setSizes] = useState<Record<string, { tables: number; bytes: number }> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [showSystem, setShowSystem] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "name", desc: false });
  const [collations, setCollations] = useState<string[]>([]);
  const [newName, setNewName] = useState("");
  const [newCollation, setNewCollation] = useState("");

  useEffect(() => {
    let live = true;
    setSizes(null);
    adminApi.databases(props.connectionId).then((r) => { if (live) { setRows(r); setError(null); setChecked(new Set()); } }).catch((e) => live && setError(describeError(e)));
    adminApi.sizes(props.connectionId).then((r) => live && setSizes(Object.fromEntries(r.map(([n, t, b]) => [n, { tables: t, bytes: b }])))).catch(() => {});
    return () => { live = false; };
  }, [props.connectionId, props.refreshKey]);
  useEffect(() => {
    schemaApi.serverOptions(props.connectionId).then((o) => setCollations(o.charsets.flatMap(([, c]) => c))).catch(() => {});
  }, [props.connectionId]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = (rows ?? []).filter((r) => (showSystem || !r.system) && (!q || r.name.toLowerCase().includes(q)));
    const val = (r: DatabaseRow): string | number => sort.key === "name" ? r.name.toLowerCase() : sort.key === "collation" ? r.collation : sort.key === "tables" ? sizes?.[r.name]?.tables ?? -1 : sizes?.[r.name]?.bytes ?? -1;
    return [...list].sort((a, b) => (val(a) < val(b) ? -1 : val(a) > val(b) ? 1 : 0) * (sort.desc ? -1 : 1));
  }, [rows, query, showSystem, sort, sizes]);
  const selectable = shown.filter((r) => !r.system);
  const allChecked = selectable.length > 0 && selectable.every((r) => checked.has(r.name));
  const toggle = (name: string) => setChecked((c) => { const n = new Set(c); n.has(name) ? n.delete(name) : n.add(name); return n; });
  const head = (key: SortKey, label: string, num = false) => (
    <th className={`sortable ${num ? "num" : ""}`} onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : false }))}>
      {label}{sort.key === key && <span className="sortmark"> {sort.desc ? "▼" : "▲"}</span>}
    </th>
  );
  const picked = [...checked].filter((n) => rows?.some((r) => r.name === n && !r.system));
  const pickedBytes = picked.reduce((a, n) => a + (sizes?.[n]?.bytes ?? 0), 0);
  const pickedTables = picked.reduce((a, n) => a + (sizes?.[n]?.tables ?? 0), 0);

  return (
    <div className="admin-page">
      <h2>Databases</h2>
      <section className="admin-card">
        <h4>Create database</h4>
        <div className="admin-row">
          <input placeholder="Database name" value={newName} onChange={(e) => setNewName(e.target.value)} disabled={props.readOnly} aria-label="New database name"
                 onKeyDown={(e) => e.key === "Enter" && newName.trim() && props.onCreate(newName.trim(), newCollation || null)} />
          <Select value={newCollation} onChange={(e) => setNewCollation(e.target.value)} disabled={props.readOnly} aria-label="Collation">
            <option value="">(server default)</option>
            {collations.map((c) => <option key={c} value={c}>{c}</option>)}
          </Select>
          <button className="primary" disabled={props.readOnly || !newName.trim()} onClick={() => props.onCreate(newName.trim(), newCollation || null)}>Create</button>
        </div>
      </section>

      <div className="admin-bar">
        <label className="check"><input type="checkbox" checked={allChecked} disabled={selectable.length === 0} onChange={(e) => setChecked(e.target.checked ? new Set(selectable.map((r) => r.name)) : new Set())} /> Check all</label>
        <button className="danger" disabled={props.readOnly || picked.length === 0}
                title={props.readOnly ? "This connection is read-only" : "Drop the selected databases (the SQL is shown first)"}
                onClick={() => props.onDrop(picked, `${picked.length} database${picked.length === 1 ? "" : "s"}${sizes ? `, ${pickedTables.toLocaleString()} tables, ${formatBytes(pickedBytes)}` : ""} will be permanently deleted.`)}>
          <Icon name="trash" /> Drop{picked.length ? ` (${picked.length})` : ""}
        </button>
        <span className="grow" />
        <label className="check"><input type="checkbox" checked={showSystem} onChange={(e) => setShowSystem(e.target.checked)} /> Show system databases</label>
        <input className="admin-search" placeholder="Search databases…" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Search databases" />
      </div>

      {error && <div className="banner bad">{error}</div>}
      {!rows && !error && <div className="empty">Loading…</div>}
      {rows && (
        <table className="info admin-table" aria-label="Databases">
          <thead><tr><th style={{ width: 26 }} />{head("name", "Database")}{head("collation", "Collation")}{head("tables", "Tables", true)}{head("size", "Size", true)}</tr></thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.name} className={checked.has(r.name) ? "picked" : ""}>
                <td><input type="checkbox" checked={checked.has(r.name)} disabled={r.system} onChange={() => toggle(r.name)} aria-label={`Select ${r.name}`} /></td>
                <td><button className="link" onClick={() => props.onOpenDatabase(r.name)}><Icon name="database" size={13} className="ico-db" /> {r.name}</button>{r.system && <span className="dim small"> system</span>}</td>
                <td className="mono">{r.collation}</td>
                <td className="num">{sizes ? (sizes[r.name]?.tables ?? 0).toLocaleString() : <span className="dim">…</span>}</td>
                <td className="num">{sizes ? formatBytes(sizes[r.name]?.bytes ?? 0) : <span className="dim">…</span>}</td>
              </tr>
            ))}
            {shown.length === 0 && <tr><td colSpan={5} className="empty small">No databases match</td></tr>}
          </tbody>
          <tfoot><tr><td /><td><b>{shown.filter((r) => !r.system).length} databases</b></td><td />
            <td className="num"><b>{sizes ? shown.reduce((a, r) => a + (sizes[r.name]?.tables ?? 0), 0).toLocaleString() : ""}</b></td>
            <td className="num"><b>{sizes ? formatBytes(shown.reduce((a, r) => a + (sizes[r.name]?.bytes ?? 0), 0)) : ""}</b></td></tr></tfoot>
        </table>
      )}
    </div>
  );
}
