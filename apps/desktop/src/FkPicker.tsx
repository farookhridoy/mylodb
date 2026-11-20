import { useEffect, useState } from "react";
import { dataApi, type ForeignKey, type LookupItem } from "./api";

/** Pick a valid value of a foreign key from the referenced table. */
export default function FkPicker(props: { connectionId: number; fk: ForeignKey; onPick(value: string): void; onClose(): void }) {
  const [search, setSearch] = useState("");
  const [items, setItems] = useState<LookupItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => {
      dataApi.lookup(props.connectionId, props.fk.ref_schema, props.fk.ref_table, props.fk.ref_columns[0], search)
        .then((r) => live && (setItems(r), setError(null)))
        .catch((e) => live && setError(String(e)));
    }, 200);
    return () => { live = false; clearTimeout(t); };
  }, [search, props.connectionId, props.fk]);

  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()}>
        <h3>{props.fk.ref_table}.{props.fk.ref_columns[0]}</h3>
        <input autoFocus placeholder="Search…" value={search} onChange={(e) => setSearch(e.target.value)} onKeyDown={(e) => e.key === "Escape" && props.onClose()} />
        {error && <div className="error">{error}</div>}
        <ul className="list pick">
          {items.map((it) => (
            <li key={it.value} onClick={() => props.onPick(it.value)}>
              <span><b>{it.value}</b>{it.label && <span className="dim"> — {it.label}</span>}</span>
            </li>
          ))}
          {items.length === 0 && !error && <div className="empty">No matching values</div>}
        </ul>
        <div className="actions"><button onClick={props.onClose}>Cancel</button></div>
      </div>
    </div>
  );
}
