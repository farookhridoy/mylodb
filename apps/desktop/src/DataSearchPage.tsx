import { useEffect, useRef, useState } from "react";
import Select from "./Select";
import { describeError, onJobProgress, searchApi, type SearchResult } from "./api";

const uid = () => Math.random().toString(36).slice(2);

/** Data search: find a value in any table of a database. Hits link to the table, filtered to the matching column. */
export default function DataSearchPage(props: {
  connectionId: number;
  database: string;
  databases: string[];
  /** Open a table with one column filter (op `contains` or `eq`). */
  onOpen(db: string, table: string, column: string, op: "contains" | "eq", text: string): void;
}) {
  const [db, setDb] = useState(props.database);
  const [needle, setNeedle] = useState("");
  const [exact, setExact] = useState(false);
  const [allColumns, setAllColumns] = useState(false);
  const [busy, setBusy] = useState<{ job: string; done: number; total: number } | null>(null);
  const [result, setResult] = useState<(SearchResult & { searched: string; db: string; exact: boolean }) | null>(null);
  const [error, setError] = useState<string | null>(null);
  const unlisten = useRef<(() => void) | null>(null);
  useEffect(() => () => unlisten.current?.(), []);

  const run = async () => {
    const job = `search-${uid()}`;
    setBusy({ job, done: 0, total: 0 }); setError(null); setResult(null);
    unlisten.current = await onJobProgress(job, (p) => setBusy({ job, done: p.done, total: p.total ?? 0 }));
    try {
      const r = await searchApi.run(props.connectionId, job, db, { needle, exact, all_columns: allColumns });
      setResult({ ...r, searched: needle, db, exact });
    } catch (e) { setError(describeError(e)); }
    unlisten.current?.(); unlisten.current = null;
    setBusy(null);
  };

  return (
    <div className="admin-page">
      <h2>Data search</h2>
      <p className="dim small">Finds a value in any table of a database. Matching follows each column's collation (usually case-insensitive); <code>%</code> and <code>_</code> in the text are taken literally. Every table is read once, so a large database can take a while: you can cancel.</p>
      <div className="compare-pickers">
        <label>Database
          <Select value={db} onChange={(e) => setDb(e.target.value)}>{props.databases.map((d) => <option key={d} value={d}>{d}</option>)}</Select>
        </label>
        <label className="grow">Search for
          <input value={needle} autoFocus placeholder="text or number" onChange={(e) => setNeedle(e.target.value)} onKeyDown={(e) => e.key === "Enter" && needle && !busy && run()} />
        </label>
        {busy ? <button onClick={() => searchApi.cancel(busy.job)}>Cancel</button> : <button className="primary" disabled={!needle || !db} onClick={run}>Search</button>}
      </div>
      <div className="check-row">
        <label className="check"><input type="checkbox" checked={exact} onChange={(e) => setExact(e.target.checked)} /> Whole value (equals) instead of contains</label>
        <label className="check"><input type="checkbox" checked={allColumns} onChange={(e) => setAllColumns(e.target.checked)} /> Also numbers, dates and other non-text columns (slower)</label>
      </div>
      {busy && <p aria-label="Progress">Searching… {busy.done} of {busy.total || "?"} tables</p>}
      {error && <div className="banner bad">{error}</div>}
      {result && (
        <>
          <p aria-label="Summary">
            {result.hits.length === 0 ? `No match for “${result.searched}” in ${result.tables_scanned} of ${result.tables_total} tables of ${result.db}.` : `“${result.searched}” found in ${result.hits.length} of ${result.tables_total} tables of ${result.db}.`}
            {result.cancelled ? " Cancelled: the rest of the tables were not searched." : ""}
          </p>
          {result.hits.length > 0 && (
            <table className="info admin-table" aria-label="Matches">
              <thead><tr><th>Table</th><th className="num">Rows</th><th>Columns</th></tr></thead>
              <tbody>
                {result.hits.map((h) => (
                  <tr key={h.table}>
                    <td className="mono">{h.table}</td>
                    <td className="num">{h.rows.toLocaleString()}</td>
                    <td>{h.columns.length === 0 ? <span className="dim">(see the table)</span> : h.columns.map(([c, n]) => (
                      <button key={c} className="link chip-link" title={`Open ${h.table} filtered to ${c}`} onClick={() => props.onOpen(result.db, h.table, c, result.exact ? "eq" : "contains", result.searched)}>{c} <span className="dim">({n.toLocaleString()})</span></button>
                    ))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {result.skipped.length > 0 && <details><summary className="dim small">{result.skipped.length} table(s) not searched</summary><ul className="small">{result.skipped.map((s) => <li key={s}>{s}</li>)}</ul></details>}
        </>
      )}
    </div>
  );
}
