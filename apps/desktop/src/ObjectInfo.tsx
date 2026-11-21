import { formatBytes } from "./browserMeta";
import type { Node, TableStat } from "./api";
import { useTableOverview } from "./tableOverview";

const KINDS: [Node["kind"], string][] = [["table", "Tables"], ["view", "Views"], ["procedure", "Procedures"], ["function", "Functions"], ["trigger", "Triggers"], ["event", "Events"]];

/** Facts about what is selected in the object tree: shown in the Info tab of the results pane (not in the sidebar, which is for the tree). */
export default function ObjectInfo(props: { connectionId: number; db: string | null; table: string | null; index: Node[]; stats: Record<string, Record<string, TableStat>> }) {
  if (props.db === null) return <div className="empty small">Select a database or table in the tree.</div>;
  return props.table
    ? <TableFacts connectionId={props.connectionId} db={props.db} table={props.table} />
    : <DatabaseFacts db={props.db} index={props.index} stats={props.stats[props.db]} />;
}

function DatabaseFacts({ db, index, stats }: { db: string; index: Node[]; stats?: Record<string, TableStat> }) {
  const mine = index.filter((n) => n.database === db);
  const sizes = stats ? Object.values(stats) : null;
  return (
    <table className="info kv">
      <tbody>
        {KINDS.map(([k, label]) => { const n = mine.filter((x) => x.kind === k).length; return n ? <tr key={k}><th>{label}</th><td>{n.toLocaleString()}</td></tr> : null; })}
        {sizes && <tr><th>Size</th><td>{formatBytes(sizes.reduce((a, s) => a + (s.bytes ?? 0), 0))}</td></tr>}
        {sizes && <tr><th>Rows (estimate)</th><td>{sizes.reduce((a, s) => a + (s.rows ?? 0), 0).toLocaleString()}</td></tr>}
      </tbody>
    </table>
  );
}

function TableFacts({ connectionId, db, table }: { connectionId: number; db: string; table: string }) {
  const { data: o, error } = useTableOverview(connectionId, db, table);
  if (!o) return <div className={error ? "error small" : "empty small"}>{error ?? "Loading…"}</div>;
  const i = o.info;
  const rows: [string, string | null][] = [
    [i.is_view ? "Type" : "Engine", i.is_view ? "View" : i.engine],
    ["Rows (estimate)", i.rows?.toLocaleString() ?? null],
    ["Size", i.data_length === null ? null : formatBytes((i.data_length ?? 0) + (i.index_length ?? 0))],
    ["Collation", i.collation],
    ["Auto-increment", i.auto_increment?.toLocaleString() ?? null],
    ["Created", i.created],
    ["Updated", i.updated],
    ["Columns", `${o.columns.length} · ${o.indexes.length} index${o.indexes.length === 1 ? "" : "es"} · ${o.outgoing.length} foreign key${o.outgoing.length === 1 ? "" : "s"}`],
    ["Comment", i.comment || null],
  ];
  return <table className="info kv"><tbody>{rows.filter(([, v]) => v).map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}</tbody></table>;
}
