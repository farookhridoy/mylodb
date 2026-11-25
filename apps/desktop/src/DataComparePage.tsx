import { useEffect, useMemo, useState } from "react";
import { api, syncApi, plainError, adminApi, queryApi, type Cells, type ColumnDetail, type DataCompareRequest, type DataCompareResult, type SavedConnection } from "./api";

interface Side { conn: number; db: string; table: string }

/** Pick a connection, a database and a table. Lists are read from the server when the choice changes. */
function Picker({ label, side, set, conns }: { label: string; side: Side; set(s: Side): void; conns: SavedConnection[] }) {
  const [dbs, setDbs] = useState<string[]>([]);
  const [tables, setTables] = useState<string[]>([]);
  useEffect(() => { queryApi.databases(side.conn).then(setDbs).catch(() => setDbs([])); }, [side.conn]);
  useEffect(() => {
    if (!side.db) { setTables([]); return; }
    adminApi.tables(side.conn, side.db).then((t) => setTables(t.filter((x) => !x.is_view).map((x) => x.name))).catch(() => setTables([]));
  }, [side.conn, side.db]);
  return (
    <fieldset className="dc-side"><legend>{label}</legend>
      <label>Connection<select value={side.conn} onChange={(e) => set({ conn: Number(e.target.value), db: "", table: "" })} aria-label={`${label} connection`}>
        {conns.map((c) => <option key={c.id} value={c.id!}>{c.name}</option>)}</select></label>
      <label>Database<select value={side.db} onChange={(e) => set({ ...side, db: e.target.value, table: "" })} aria-label={`${label} database`}>
        <option value="">(choose)</option>{dbs.map((d) => <option key={d} value={d}>{d}</option>)}</select></label>
      <label>Table<select value={side.table} onChange={(e) => set({ ...side, table: e.target.value })} aria-label={`${label} table`}>
        <option value="">(choose)</option>{tables.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
    </fieldset>
  );
}

const show = (v: string | null) => (v === null ? <i className="dim">NULL</i> : v.length > 60 ? v.slice(0, 60) + "…" : v);

function Sample({ title, columns, rows, empty }: { title: string; columns: string[]; rows: Cells[]; empty: string }) {
  return (
    <details className="dc-sample" open={rows.length > 0}>
      <summary>{title}</summary>
      {rows.length === 0 ? <div className="dim small">{empty}</div> : (
        <div className="dc-scroll"><table className="grid"><thead><tr>{columns.map((c) => <th key={c}>{c}</th>)}</tr></thead>
          <tbody>{rows.map((r, i) => <tr key={i}>{r.map((v, k) => <td key={k}>{show(v)}</td>)}</tr>)}</tbody></table></div>
      )}
    </details>
  );
}

/** Compare the rows of two tables (by primary key) and make the target match the source. */
export default function DataComparePage(props: { connections: SavedConnection[]; activeId: number; preset?: { db: string; table: string }; onStatus(msg: string): void; onChanged(): void }) {
  const first = props.connections.find((c) => c.id === props.activeId) ?? props.connections[0];
  const other = props.connections.find((c) => c.id !== first?.id) ?? first;
  const [source, setSource] = useState<Side>({ conn: first?.id ?? 0, db: props.preset?.db ?? "", table: props.preset?.table ?? "" });
  const [target, setTarget] = useState<Side>({ conn: other?.id ?? 0, db: props.preset?.db ?? "", table: props.preset?.table ?? "" });
  const [columns, setColumns] = useState<ColumnDetail[]>([]);
  const [key, setKey] = useState<string[]>([]);
  const [where, setWhere] = useState("");
  const [del, setDel] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [res, setRes] = useState<DataCompareResult | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [noFk, setNoFk] = useState(false);
  const [typed, setTyped] = useState("");
  const env = props.connections.find((c) => c.id === target.conn)?.environment ?? "development";

  // the key defaults to the source's primary key
  useEffect(() => {
    setRes(null);
    if (!source.db || !source.table) { setColumns([]); setKey([]); return; }
    api.columnDetails(source.conn, source.db, source.table).then((cols) => { setColumns(cols); setKey(cols.filter((c) => c.key === "PRI").map((c) => c.name)); }).catch(() => setColumns([]));
  }, [source.conn, source.db, source.table]);

  const request = useMemo((): DataCompareRequest => ({
    source_id: source.conn, source_db: source.db, source_table: source.table, target_id: target.conn, target_db: target.db, target_table: target.table,
    key, where_sql: where.trim() || null, delete_extra: del,
  }), [source, target, key, where, del]);
  const ready = !!(source.db && source.table && target.db && target.table && key.length);
  const same = source.conn === target.conn && source.db === target.db && source.table === target.table;

  const compare = async () => {
    setBusy(true); setError(null); setRes(null);
    try { setRes(await syncApi.compare(request)); } catch (e) { setError(plainError(e)); }
    setBusy(false);
  };
  const apply = async () => {
    setBusy(true); setError(null);
    try {
      const done = await syncApi.apply(request, noFk);
      props.onStatus(`Synced ${target.db}.${target.table}: ${done.inserted} inserted, ${done.updated} updated, ${done.deleted} deleted.`);
      props.onChanged();
      setConfirm(false); setTyped("");
      setRes(await syncApi.compare(request));
    } catch (e) { setError(plainError(e)); }
    setBusy(false);
  };
  const changes = res ? res.only_source_count + res.changed_count + (del ? res.only_target_count : 0) : 0;
  const needWord = env === "production";

  return (
    <div className="datacompare">
      <p className="dim small">Compares the rows of two tables on a key and writes the statements that make the <b>target</b> match the <b>source</b>. MySQL and MariaDB. Both tables are read into memory (up to a million rows each): narrow it with a condition for bigger ones.</p>
      <div className="dc-pair"><Picker label="Source" side={source} set={setSource} conns={props.connections} /><Picker label="Target" side={target} set={setTarget} conns={props.connections} /></div>
      <fieldset className="dc-opts"><legend>Compare on</legend>
        <div className="dc-key">{columns.length === 0 ? <span className="dim small">Choose the source table.</span> : columns.map((c) => (
          <label key={c.name} className="check"><input type="checkbox" checked={key.includes(c.name)} onChange={(e) => setKey(e.target.checked ? [...key, c.name] : key.filter((k) => k !== c.name))} /> {c.name}{c.key === "PRI" ? " (primary key)" : ""}</label>))}</div>
        <label>Only rows where (optional, applied to both)<input value={where} onChange={(e) => setWhere(e.target.value)} placeholder="created_at >= '2026-01-01'" spellCheck={false} /></label>
        <label className="check"><input type="checkbox" checked={del} onChange={(e) => setDel(e.target.checked)} /> Also delete the rows only the target has</label>
      </fieldset>
      {same && <div className="banner">The source and the target are the same table.</div>}
      {error && <div className="banner bad" role="alert">{error}</div>}
      <div className="actions"><button className="primary" disabled={!ready || busy || same} onClick={compare}>{busy && !confirm ? "Comparing…" : "Compare"}</button></div>

      {res && (
        <div className="dc-result" aria-label="Comparison">
          <div className="dc-chips">
            <span className="chip-same"><b>{res.same.toLocaleString()}</b> identical</span>
            <span className="chip-changed"><b>{res.changed_count.toLocaleString()}</b> different</span>
            <span className="chip-new"><b>{res.only_source_count.toLocaleString()}</b> only in the source</span>
            <span className="chip-old"><b>{res.only_target_count.toLocaleString()}</b> only in the target</span>
          </div>
          {res.skipped.length > 0 && <div className="banner">Not compared (in one table only): {res.skipped.join(", ")}</div>}
          <Sample title={`Only in the source: will be inserted (${res.only_source_count})`} columns={res.columns} rows={res.only_source} empty="None." />
          <details className="dc-sample" open={res.changed.length > 0}>
            <summary>Different: will be updated ({res.changed_count})</summary>
            {res.changed.length === 0 ? <div className="dim small">None.</div> : (
              <div className="dc-scroll"><table className="grid"><thead><tr><th>Key ({res.key.join(", ")})</th><th>Column</th><th>Source</th><th>Target now</th></tr></thead>
                <tbody>{res.changed.flatMap((c, i) => c.diffs.map((d, k) => <tr key={`${i}-${k}`}>{k === 0 ? <td rowSpan={c.diffs.length}>{c.key.map(show)}</td> : null}<td>{d.column}</td><td>{show(d.source)}</td><td>{show(d.target)}</td></tr>))}</tbody></table></div>
            )}
          </details>
          <Sample title={`Only in the target: ${del ? "will be deleted" : "left alone"} (${res.only_target_count})`} columns={res.columns} rows={res.only_target} empty="None." />
          {res.statements_total > 0 && (
            <details className="dc-sample"><summary>SQL that will run on the target ({res.statements_total}{res.statements_total > res.statements.length ? `, first ${res.statements.length} shown` : ""})</summary>
              <pre className="preview-sql">{res.statements.join(";\n\n") + ";"}</pre></details>
          )}
          <div className="actions">
            {changes === 0 ? <span className="dim">The target already matches the source{del || res.only_target_count === 0 ? "." : " (rows only the target has are kept)."}</span> : (
              <button className="primary danger-ish" onClick={() => setConfirm(true)} disabled={busy}>Make the target match… ({changes.toLocaleString()} change{changes === 1 ? "" : "s"})</button>
            )}
          </div>
        </div>
      )}

      {confirm && res && (
        <div className="overlay" onMouseDown={() => !busy && setConfirm(false)}>
          <div className="dialog" role="dialog" aria-label="Make the target match" onMouseDown={(e) => e.stopPropagation()}>
            <h3>Change {target.db}.{target.table}?</h3>
            <p>{res.only_source_count} row(s) inserted, {res.changed_count} updated{del ? `, ${res.only_target_count} deleted` : ""}, in <b>{props.connections.find((c) => c.id === target.conn)?.name}</b>. It runs in one transaction: if anything fails, nothing changes.</p>
            <label className="check"><input type="checkbox" checked={noFk} onChange={(e) => setNoFk(e.target.checked)} /> Switch off foreign key checks while it runs</label>
            {needWord && <label>This is a production connection. Type <b>{target.table}</b> to continue<input value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus /></label>}
            <div className="actions"><button onClick={() => setConfirm(false)} disabled={busy}>Cancel</button><button className="primary" onClick={apply} disabled={busy || (needWord && typed !== target.table)}>{busy ? "Applying…" : "Apply changes"}</button></div>
          </div>
        </div>
      )}
    </div>
  );
}
