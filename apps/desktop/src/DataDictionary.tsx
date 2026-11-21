import { useEffect, useState } from "react";
import { api, describeError, type TableOverview } from "./api";

/** Every table of a database with its columns, keys and foreign keys, laid out to print (phpMyAdmin's "Data dictionary"). */
export default function DataDictionary(props: { connectionId: number; db: string; tables: string[]; onClose(): void }) {
  const [docs, setDocs] = useState<{ name: string; o: TableOverview }[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    Promise.all(props.tables.map(async (name) => ({ name, o: await api.tableOverview(props.connectionId, props.db, name) })))
      .then((d) => live && setDocs(d)).catch((e) => live && setError(describeError(e)));
    return () => { live = false; };
  }, [props.connectionId, props.db, props.tables]);
  return (
    <div className="overlay dict-overlay">
      <div className="dialog dict" role="dialog" aria-label="Data dictionary">
        <div className="dict-bar no-print"><h3>Data dictionary: {props.db}</h3><span className="grow" />
          <button className="primary" disabled={!docs} onClick={() => window.print()}>Print</button><button onClick={props.onClose}>Close</button></div>
        {error && <div className="banner bad">{error}</div>}
        {!docs && !error && <div className="dim">Reading {props.tables.length} tables…</div>}
        <div className="dict-body">
          <h2 className="print-only">{props.db}</h2>
          {docs?.map(({ name, o }) => (
            <section key={name} className="dict-table">
              <h4>{name} <span className="dim small">{o.info.engine ?? ""}{o.info.comment ? ` · ${o.info.comment}` : ""}</span></h4>
              <table className="info">
                <thead><tr><th>Column</th><th>Type</th><th>Null</th><th>Key</th><th>Default</th><th>Extra</th><th>Comment</th></tr></thead>
                <tbody>{o.columns.map((c) => (
                  <tr key={c.name}><td>{c.name}</td><td className="mono">{c.column_type}</td><td>{c.nullable ? "Yes" : "No"}</td><td>{c.key}</td><td>{c.default ?? ""}</td><td>{c.extra}</td><td>{c.comment}</td></tr>))}</tbody>
              </table>
              {o.outgoing.length > 0 && <div className="small">Links to: {o.outgoing.map((f) => `${f.columns.join(", ")} → ${f.ref_table}(${f.ref_columns.join(", ")})`).join("; ")}</div>}
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
