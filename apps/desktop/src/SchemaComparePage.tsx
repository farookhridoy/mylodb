import { Fragment, useEffect, useMemo, useState } from "react";
import Select from "./Select";
import { adminApi, compareApi, describeError, type SchemaCompare, type TableChange } from "./api";

export interface OpenConnection { id: number; name: string }

const LABEL = { added: "New in source", changed: "Different", removed: "Only in target" } as const;

/** Statements for the chosen tables in an order that works: creates and alters, the new tables' foreign keys, then drops (mirrors `SchemaDiff::script`). */
export function scriptFor(changes: TableChange[], chosen: Set<string>): string[] {
  const picked = changes.filter((c) => chosen.has(c.table));
  const out = [
    ...picked.filter((c) => c.kind !== "removed").flatMap((c) => c.statements),
    ...picked.flatMap((c) => c.deferred),
    ...picked.filter((c) => c.kind === "removed").flatMap((c) => c.statements),
  ];
  return out.map((s) => (s.endsWith(";") ? s : `${s};`));
}

/**
 * Schema compare: pick a source database (on any open connection) and make this connection's database match it.
 * Nothing runs here: "Preview and apply" hands the chosen statements to the Preview SQL dialog.
 */
export default function SchemaComparePage(props: {
  /** The target: the connection this tab belongs to. */
  connectionId: number;
  targetDb: string;
  databases: string[];
  connections: OpenConnection[];
  onApply(statements: string[], summary: string): void;
}) {
  const [sourceConn, setSourceConn] = useState(props.connectionId);
  const [sourceDbs, setSourceDbs] = useState<string[]>([]);
  const [sourceDb, setSourceDb] = useState("");
  const [targetDb, setTargetDb] = useState(props.targetDb);
  const [diff, setDiff] = useState<SchemaCompare | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setSourceDb(""); setDiff(null);
    adminApi.databases(sourceConn).then((rows) => { if (live) setSourceDbs(rows.filter((r) => !r.system).map((r) => r.name)); }).catch((e) => live && setError(describeError(e)));
    return () => { live = false; };
  }, [sourceConn]);

  const run = async () => {
    setBusy(true); setError(null); setDiff(null);
    try {
      const d = await compareApi.schemas(sourceConn, sourceDb, props.connectionId, targetDb);
      setDiff(d);
      setChosen(new Set(d.changes.filter((c) => !c.destructive).map((c) => c.table))); // drops are never pre-selected
      setOpen(new Set());
    } catch (e) { setError(describeError(e)); }
    setBusy(false);
  };

  const script = useMemo(() => (diff ? scriptFor(diff.changes, chosen) : []), [diff, chosen]);
  const toggle = (set: Set<string>, k: string) => { const n = new Set(set); n.has(k) ? n.delete(k) : n.add(k); return n; };
  const dropsChosen = diff?.changes.filter((c) => c.destructive && chosen.has(c.table)).length ?? 0;

  return (
    <div className="admin-page">
      <h2>Compare schemas</h2>
      <p className="dim small">Makes the tables of the <b>target</b> database match the <b>source</b>. Only tables are compared (not views, routines, triggers or events). Nothing changes until you preview and run the statements.</p>
      <div className="compare-pickers">
        <label>Source connection
          <Select value={String(sourceConn)} onChange={(e) => setSourceConn(Number(e.target.value))}>
            {props.connections.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </Select>
        </label>
        <label>Source database
          <Select value={sourceDb} onChange={(e) => { setSourceDb(e.target.value); setDiff(null); }}>
            <option value="">Choose a database…</option>
            {sourceDbs.map((d) => <option key={d} value={d}>{d}</option>)}
          </Select>
        </label>
        <label>Target database (this connection)
          <Select value={targetDb} onChange={(e) => { setTargetDb(e.target.value); setDiff(null); }}>
            {props.databases.map((d) => <option key={d} value={d}>{d}</option>)}
          </Select>
        </label>
        <button className="primary" disabled={!sourceDb || !targetDb || busy} onClick={run}>{busy ? "Comparing…" : "Compare"}</button>
      </div>
      {error && <div className="banner bad">{error}</div>}
      {diff && (
        <>
          {diff.warnings.length > 0 && <div className="banner">{diff.warnings.length} note(s) from reading the tables, e.g. {diff.warnings[0]}. The designer cannot represent those parts, so differences there are not compared.</div>}
          <p aria-label="Summary">{diff.changes.length === 0 ? `The schemas match: ${diff.identical} table${diff.identical === 1 ? "" : "s"}, no differences.` : `${diff.changes.length} difference${diff.changes.length === 1 ? "" : "s"}, ${diff.identical} table${diff.identical === 1 ? "" : "s"} identical.`}</p>
          {diff.changes.length > 0 && (
            <table className="info admin-table" aria-label="Differences">
              <thead><tr><th /><th>Table</th><th>Difference</th><th /></tr></thead>
              <tbody>
                {diff.changes.map((c) => (
                  <Fragment key={c.table}>
                    <tr className={c.destructive ? "bad" : ""}>
                      <td><input type="checkbox" aria-label={`Include ${c.table}`} checked={chosen.has(c.table)} onChange={() => setChosen(toggle(chosen, c.table))} /></td>
                      <td className="mono">{c.table}</td>
                      <td>{LABEL[c.kind]}{c.destructive ? " · drops the table and its data" : ""}</td>
                      <td><button className="link" onClick={() => setOpen(toggle(open, c.table))}>{open.has(c.table) ? "Hide SQL" : "Show SQL"}</button></td>
                    </tr>
                    {open.has(c.table) && <tr><td /><td colSpan={3}><pre className="preview">{[...c.statements, ...c.deferred].map((s) => (s.endsWith(";") ? s : `${s};`)).join("\n")}</pre></td></tr>}
                  </Fragment>
                ))}
              </tbody>
            </table>
          )}
          {diff.changes.length > 0 && (
            <div className="actions" style={{ justifyContent: "flex-start" }}>
              <button className="primary" disabled={script.length === 0} onClick={() => props.onApply(script, `${chosen.size} table(s) of ${targetDb} made to match ${sourceDb}${dropsChosen ? `, including ${dropsChosen} table drop(s)` : ""}`)}>Preview and apply…</button>
              <span className="dim small">{script.length} statement(s) for {chosen.size} table(s)</span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
