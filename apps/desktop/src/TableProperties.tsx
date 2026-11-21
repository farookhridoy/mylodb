import { useState } from "react";
import Icon from "./Icon";
import { formatBytes } from "./browserMeta";
import type { ForeignKeyInfo, TableOverview } from "./api";
import { useTableOverview } from "./tableOverview";

type Section = "columns" | "constraints" | "foreign_keys" | "references" | "triggers" | "indexes" | "partitions" | "statistics" | "ddl";

const fkRule = (r: string) => r || "—";

/** Table "Properties" view: settings, columns, constraints, keys in both directions, triggers, indexes, partitions, statistics and DDL. */
export default function TableProperties(props: {
  connectionId: number; db: string; table: string;
  onOpenTable(db: string, table: string): void;
  onDesign(): void;
  onOpenInConsole(sql: string): void;
}) {
  const { data: o, loading, error, refresh } = useTableOverview(props.connectionId, props.db, props.table);
  const [section, setSection] = useState<Section>("columns");

  if (!o) return <div className="props"><div className="empty">{error ? <span className="error">{error}</span> : "Loading…"}</div></div>;
  if (o.columns.length === 0) return <div className="props"><div className="empty">{props.db}.{props.table} was not found. It may have been dropped or renamed.</div></div>;

  const nav: { id: Section; label: string; count?: number; icon: "column" | "unique" | "link" | "trigger" | "index" | "folder" | "info" | "function" }[] = [
    { id: "columns", label: "Columns", count: o.columns.length, icon: "column" },
    { id: "constraints", label: "Constraints", count: o.constraints.length, icon: "unique" },
    { id: "foreign_keys", label: "Foreign Keys", count: o.outgoing.length, icon: "link" },
    { id: "references", label: "References", count: o.incoming.length, icon: "link" },
    { id: "triggers", label: "Triggers", count: o.triggers.length, icon: "trigger" },
    { id: "indexes", label: "Indexes", count: o.indexes.length, icon: "index" },
    { id: "partitions", label: "Partitions", count: o.partitions.length, icon: "folder" },
    { id: "statistics", label: "Statistics", icon: "info" },
    { id: "ddl", label: "DDL", icon: "function" },
  ];

  return (
    <div className="props">
      <div className="props-head">
        <label>Table name<input readOnly value={props.table} /></label>
        <label>Engine<input readOnly value={o.info.engine ?? (o.info.is_view ? "(view)" : "")} /></label>
        <label>Collation<input readOnly value={o.info.collation ?? ""} /></label>
        <label>Charset<input readOnly value={o.info.charset ?? ""} /></label>
        <label>Auto increment<input readOnly value={o.info.auto_increment ?? ""} /></label>
        <label className="check"><input type="checkbox" disabled checked={o.info.partitioned} /> Partitioned</label>
        <label className="wide">Description<input readOnly value={o.info.comment} placeholder="(none)" /></label>
        <div className="props-actions">
          {!o.info.is_view && <button onClick={props.onDesign} title="Edit columns, indexes and keys with a preview of the SQL"><Icon name="edit" /> Design table…</button>}
          <button onClick={refresh} disabled={loading} title="Read the table again"><Icon name="refresh" /> Refresh</button>
        </div>
      </div>
      <div className="props-body">
        <nav className="props-nav" aria-label="Table sections">
          {nav.map((n) => (
            <button key={n.id} className={section === n.id ? "active" : ""} onClick={() => setSection(n.id)}>
              <Icon name={n.icon} size={12} /> {n.label}{n.count !== undefined && <span className="count">{n.count}</span>}
            </button>
          ))}
        </nav>
        <div className="props-pane">
          {section === "columns" && (
            <table className="info">
              <thead><tr><th>Column name</th><th className="num">#</th><th>Data type</th><th>Not null</th><th>Auto increment</th><th>Key</th><th>Default</th><th>Extra</th><th>Comment</th></tr></thead>
              <tbody>
                {o.columns.map((c) => (
                  <tr key={c.name}>
                    <td><span className="key">{c.key === "PRI" ? <Icon name="key" size={12} className="ico-pk" /> : c.key === "UNI" ? <Icon name="unique" size={11} /> : <Icon name="column" size={11} className="ico-col" />}</span> {c.name}</td>
                    <td className="num dim">{c.ordinal}</td>
                    <td className="mono">{c.column_type}</td>
                    <td>{c.nullable ? "" : <Icon name="check" size={12} />}</td>
                    <td>{/auto_increment/i.test(c.extra) ? <Icon name="check" size={12} /> : ""}</td>
                    <td>{c.key}</td>
                    <td className="mono">{c.default ?? (c.nullable ? <span className="dim">NULL</span> : "")}</td>
                    <td>{c.extra}</td>
                    <td>{c.comment}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {section === "constraints" && (
            o.constraints.length === 0 ? <div className="empty small">No constraints</div> : (
              <table className="info">
                <thead><tr><th>Name</th><th>Type</th><th>Columns</th></tr></thead>
                <tbody>{o.constraints.map((c) => <tr key={c.name + c.kind}><td>{c.name}</td><td>{c.kind}</td><td className="mono">{c.columns.join(", ")}</td></tr>)}</tbody>
              </table>
            )
          )}
          {section === "foreign_keys" && <FkTable rows={o.outgoing} mode="out" onOpen={props.onOpenTable} empty="This table does not reference other tables" />}
          {section === "references" && <FkTable rows={o.incoming} mode="in" onOpen={props.onOpenTable} empty="No other table references this one" />}
          {section === "triggers" && (
            o.triggers.length === 0 ? <div className="empty small">No triggers</div> : (
              <table className="info">
                <thead><tr><th>Name</th><th>Timing</th><th>Event</th><th>Statement</th></tr></thead>
                <tbody>{o.triggers.map((t) => <tr key={t.name + t.event}><td>{t.name}</td><td>{t.timing}</td><td>{t.event}</td><td className="mono clip" title={t.statement}>{t.statement}</td></tr>)}</tbody>
              </table>
            )
          )}
          {section === "indexes" && (
            o.indexes.length === 0 ? <div className="empty small">No indexes</div> : (
              <table className="info">
                <thead><tr><th>Name</th><th>Type</th><th>Unique</th><th>Columns</th></tr></thead>
                <tbody>{o.indexes.map((i) => <tr key={i.name}><td><span className="key">{i.name === "PRIMARY" ? <Icon name="key" size={12} className="ico-pk" /> : i.unique ? <Icon name="unique" size={11} /> : <Icon name="index" size={11} />}</span> {i.name}</td><td>{i.kind}</td><td>{i.unique ? <Icon name="check" size={12} /> : ""}</td><td className="mono">{i.columns.join(", ")}</td></tr>)}</tbody>
              </table>
            )
          )}
          {section === "partitions" && (
            o.partitions.length === 0 ? <div className="empty small">The table is not partitioned</div> : (
              <table className="info">
                <thead><tr><th>Name</th><th>Method</th><th>Expression</th><th className="num">Rows (est.)</th><th className="num">Data size</th></tr></thead>
                <tbody>{o.partitions.map((p) => <tr key={p.name}><td>{p.name}</td><td>{p.method}</td><td className="mono">{p.expression}</td><td className="num">{p.rows?.toLocaleString()}</td><td className="num">{p.data_length !== null ? formatBytes(p.data_length) : ""}</td></tr>)}</tbody>
              </table>
            )
          )}
          {section === "statistics" && <Statistics o={o} />}
          {section === "ddl" && (
            <div className="ddl">
              <div className="ddl-bar">
                <button onClick={() => navigator.clipboard?.writeText(o.ddl ?? "").catch(() => {})} disabled={!o.ddl}><Icon name="copy" /> Copy</button>
                <button onClick={() => o.ddl && props.onOpenInConsole(o.ddl)} disabled={!o.ddl}>Open in console</button>
              </div>
              {o.ddl ? <pre>{o.ddl};</pre> : <div className="empty small">The server did not return a definition (your account may not be allowed to read it).</div>}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function FkTable({ rows, mode, onOpen, empty }: { rows: ForeignKeyInfo[]; mode: "out" | "in"; onOpen(db: string, table: string): void; empty: string }) {
  if (rows.length === 0) return <div className="empty small">{empty}</div>;
  return (
    <table className="info">
      <thead>
        <tr><th>Name</th>{mode === "in" && <th>Table</th>}<th>Columns</th><th>{mode === "out" ? "Referenced table" : "Referenced columns"}</th>{mode === "out" && <th>Referenced columns</th>}<th>On delete</th><th>On update</th></tr>
      </thead>
      <tbody>
        {rows.map((f) => (
          <tr key={f.schema + f.table + f.name}>
            <td>{f.name}</td>
            {mode === "in" && <td><button className="link" onClick={() => onOpen(f.schema, f.table)}>{f.table}</button></td>}
            <td className="mono">{f.columns.join(", ")}</td>
            {mode === "out" && <td><button className="link" onClick={() => onOpen(f.ref_schema, f.ref_table)}>{f.ref_table}</button></td>}
            <td className="mono">{f.ref_columns.join(", ")}</td>
            <td>{fkRule(f.on_delete)}</td><td>{fkRule(f.on_update)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Statistics({ o }: { o: TableOverview }) {
  const i = o.info;
  const n = (v: number | null) => (v === null ? "—" : v.toLocaleString());
  const b = (v: number | null) => (v === null ? "—" : `${formatBytes(v)} (${v.toLocaleString()} bytes)`);
  const rows: [string, string][] = [
    ["Rows (estimate)", n(i.rows)], ["Average row length", i.avg_row_length === null ? "—" : `${n(i.avg_row_length)} bytes`],
    ["Data size", b(i.data_length)], ["Index size", b(i.index_length)], ["Free space", b(i.data_free)],
    ["Row format", i.row_format ?? "—"], ["Auto increment", n(i.auto_increment)], ["Created", i.created ?? "—"], ["Last updated", i.updated ?? "—"],
  ];
  return (
    <>
      <table className="info kv"><tbody>{rows.map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}</tbody></table>
      <p className="dim small note">Sizes and row counts come from the server's statistics, which MySQL 8 caches (up to a day). <code>ANALYZE TABLE</code> refreshes them.</p>
    </>
  );
}
