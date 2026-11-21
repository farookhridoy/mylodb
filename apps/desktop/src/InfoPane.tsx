import { formatBytes } from "./browserMeta";
import type { StatementResult } from "./api";
import type { RunState } from "./ResultsPane";
import { useTableOverview } from "./tableOverview";

export interface InfoContext {
  connectionId: number;
  /** `user@host` of the connection. */
  who: string;
  environment: string;
  readOnly: boolean;
  database: string | null;
  /** For a table tab: the table, so its summary can be shown. */
  table?: { db: string; name: string };
  onOpenProperties?(): void;
}

const oneLine = (sql: string) => sql.replace(/\s+/g, " ").trim();

/** The "Info" tab: where the console is running and what the last run did, statement by statement; for a table tab, a summary of the table. */
export default function InfoPane({ run, ctx }: { run: RunState | undefined; ctx: InfoContext }) {
  return (
    <div className="info-pane">
      <h4>Connection</h4>
      <table className="info kv">
        <tbody>
          <tr><th>Account</th><td>{ctx.who}</td></tr>
          <tr><th>Environment</th><td style={{ textTransform: "capitalize" }}>{ctx.environment}{ctx.readOnly ? " · read-only" : ""}</td></tr>
          <tr><th>Database</th><td>{ctx.database ?? <span className="dim">(none selected)</span>}</td></tr>
        </tbody>
      </table>
      {ctx.table && <TableSummary connectionId={ctx.connectionId} db={ctx.table.db} table={ctx.table.name} onOpenProperties={ctx.onOpenProperties} />}
      <h4>Last run</h4>
      {!run || (!run.results && !run.error) ? <div className="empty small">Nothing has been run in this console yet.</div> : (
        <>
          <table className="info">
            <thead><tr><th>#</th><th>Statement</th><th className="num">Rows</th><th className="num">Affected</th><th className="num">Warnings</th><th className="num">Time</th></tr></thead>
            <tbody>
              {(run.results ?? []).map((r, i) => <StatementRow key={i} n={i + 1} r={r} />)}
            </tbody>
          </table>
          <p className="dim small note">{(run.results ?? []).length} statement{(run.results ?? []).length === 1 ? "" : "s"} · {run.totalMs} ms in total{run.error ? ` · ${run.error}` : ""}</p>
        </>
      )}
    </div>
  );
}

function StatementRow({ n, r }: { n: number; r: StatementResult }) {
  const rows = r.sets.reduce((a, s) => a + s.total_rows, 0);
  return (
    <tr className={r.error ? "bad" : ""}>
      <td className="num dim">{n}</td>
      <td className="mono clip" title={r.sql}>{oneLine(r.sql).slice(0, 140)}{r.error && <div className="error small">{r.error}</div>}</td>
      <td className="num">{r.sets.length ? rows.toLocaleString() : ""}</td>
      <td className="num">{!r.sets.length && !r.error ? r.affected_rows.toLocaleString() : ""}</td>
      <td className="num">{r.warnings || ""}</td>
      <td className="num">{r.elapsed_ms} ms</td>
    </tr>
  );
}

function TableSummary({ connectionId, db, table, onOpenProperties }: { connectionId: number; db: string; table: string; onOpenProperties?(): void }) {
  const { data: o } = useTableOverview(connectionId, db, table);
  if (!o || o.columns.length === 0) return null;
  const size = (o.info.data_length ?? 0) + (o.info.index_length ?? 0);
  return (
    <>
      <h4>Table <button className="link" onClick={onOpenProperties}>Open Properties</button></h4>
      <table className="info kv">
        <tbody>
          <tr><th>Name</th><td>{db}.{table}</td></tr>
          <tr><th>Engine</th><td>{o.info.engine ?? (o.info.is_view ? "view" : "")}</td></tr>
          <tr><th>Rows (estimate)</th><td>{o.info.rows?.toLocaleString() ?? "—"}</td></tr>
          <tr><th>Size</th><td>{o.info.data_length === null ? "—" : formatBytes(size)}</td></tr>
          <tr><th>Columns</th><td>{o.columns.length} · {o.indexes.length} index{o.indexes.length === 1 ? "" : "es"} · {o.outgoing.length} foreign key{o.outgoing.length === 1 ? "" : "s"} · referenced by {o.incoming.length}</td></tr>
          {o.info.comment && <tr><th>Comment</th><td>{o.info.comment}</td></tr>}
        </tbody>
      </table>
    </>
  );
}

