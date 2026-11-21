import { useEffect, useState } from "react";
import { adminApi, describeError, type Grid } from "./api";

function GridTable({ g, label }: { g: Grid; label: string }) {
  if (!g.columns.length) return null;
  return (
    <table className="info admin-table" aria-label={label} style={{ maxWidth: "none" }}>
      <thead><tr>{g.columns.map((c) => <th key={c}>{c}</th>)}</tr></thead>
      <tbody>{g.rows.map((r, i) => <tr key={i}>{r.map((v, j) => <td key={j} className="mono">{v ?? <span className="dim">NULL</span>}</td>)}</tr>)}</tbody>
    </table>
  );
}

/** Engines, character sets and replication status: phpMyAdmin's Engines, Charsets and Replication tabs on one page. */
export default function CatalogPage(props: { connectionId: number }) {
  const [cat, setCat] = useState<Awaited<ReturnType<typeof adminApi.catalog>> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [part, setPart] = useState<"engines" | "charsets" | "replication">("engines");
  useEffect(() => {
    let live = true;
    adminApi.catalog(props.connectionId).then((c) => live && setCat(c)).catch((e) => live && setError(describeError(e)));
    return () => { live = false; };
  }, [props.connectionId]);
  return (
    <div className="admin-page">
      <h2>Engines, character sets, replication</h2>
      <div className="dz-tabs" role="tablist">
        {(["engines", "charsets", "replication"] as const).map((p) => <button key={p} role="tab" className={part === p ? "active" : ""} onClick={() => setPart(p)}>{p[0].toUpperCase() + p.slice(1)}</button>)}
      </div>
      {error && <div className="banner bad">{error}</div>}
      {!cat && !error && <div className="empty">Loading…</div>}
      {cat && part === "engines" && <GridTable g={cat.engines} label="Engines" />}
      {cat && part === "charsets" && <GridTable g={cat.charsets} label="Character sets" />}
      {cat && part === "replication" && (cat.replication.columns.length === 0
        ? <div className="dim" style={{ marginTop: 10 }}>{cat.replication_note ?? "This server is not a replica: it has no replication source configured."}</div>
        : <table className="info kv" aria-label="Replication"><tbody>{cat.replication.columns.map((c, i) => <tr key={c}><th>{c}</th><td className="mono">{cat.replication.rows[0]?.[i] ?? ""}</td></tr>)}</tbody></table>)}
    </div>
  );
}
