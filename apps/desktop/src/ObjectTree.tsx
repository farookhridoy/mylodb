import { useMemo, useState } from "react";
import type { Node } from "./api";
import { buildTree } from "./tree";

/** Highlight the case-insensitive substring terms of the query inside a name. */
function Highlight({ text, terms }: { text: string; terms: string[] }) {
  const lower = text.toLowerCase();
  const term = terms.find((t) => t && lower.includes(t));
  if (!term) return <>{text}</>;
  const i = lower.indexOf(term);
  return <>{text.slice(0, i)}<mark>{text.slice(i, i + term.length)}</mark>{text.slice(i + term.length)}</>;
}

export default function ObjectTree(props: {
  index: Node[];
  matches: Set<number> | null; // null = no filter active
  terms: string[];
  onOpenTable(database: string, table: string): void;
  onTableMenu(e: React.MouseEvent, database: string, table: string): void;
  onDatabaseMenu(e: React.MouseEvent, database: string | null): void;
}) {
  const [open, setOpen] = useState<Set<string>>(new Set());
  const tree = useMemo(() => buildTree(props.index, props.matches), [props.index, props.matches]);
  const filtering = props.matches !== null;
  const isOpen = (key: string) => filtering || open.has(key);
  const toggle = (key: string) => {
    const next = new Set(open);
    next.has(key) ? next.delete(key) : next.add(key);
    setOpen(next);
  };

  if (filtering && tree.length === 0) return <div className="empty">No matching objects</div>;

  return (
    <ul className="tree" onContextMenu={(e) => e.target === e.currentTarget && props.onDatabaseMenu(e, null)}>
      {tree.map((db) => {
        const total = db.groups.reduce((n, g) => n + g.items.length, 0);
        return (
          <li key={db.name}>
            <div className="row-item db" onClick={() => toggle(db.name)} onContextMenu={(e) => props.onDatabaseMenu(e, db.name)}>
              <span className="chev">{isOpen(db.name) ? "▾" : "▸"}</span>
              <span className="icon">🛢</span>
              <Highlight text={db.name} terms={props.terms} />
              {filtering && <span className="badge">{total + [...db.columns.values()].reduce((n, c) => n + c.length, 0)}</span>}
            </div>
            {isOpen(db.name) && (
              <ul>
                {db.groups.filter((g) => g.items.length > 0).map((g) => {
                  const gk = `${db.name}/${g.kind}`;
                  return (
                    <li key={gk}>
                      <div className="row-item group" onClick={() => toggle(gk)}>
                        <span className="chev">{isOpen(gk) ? "▾" : "▸"}</span>
                        {g.label}<span className="badge">{g.items.length}</span>
                      </div>
                      {isOpen(gk) && (
                        <ul>
                          {g.items.map((n) => {
                            const cols = g.kind === "table" || g.kind === "view" ? db.columns.get(n.name) : undefined;
                            const nk = `${gk}/${n.name}`;
                            return (
                              <li key={nk}>
                                <div className="row-item leaf" onClick={() => cols && toggle(nk)}
                                     onDoubleClick={() => (g.kind === "table" || g.kind === "view") && props.onOpenTable(db.name, n.name)}
                                     onContextMenu={(e) => (g.kind === "table" || g.kind === "view") && props.onTableMenu(e, db.name, n.name)}>
                                  <span className="chev">{cols ? (isOpen(nk) ? "▾" : "▸") : ""}</span>
                                  <Highlight text={n.name} terms={props.terms} />
                                  {n.parent && <span className="dim"> · {n.parent}</span>}
                                </div>
                                {cols && isOpen(nk) && (
                                  <ul>
                                    {cols.map((c) => (
                                      <li key={c.name} className="row-item col">
                                        <Highlight text={c.name} terms={props.terms} />
                                      </li>
                                    ))}
                                  </ul>
                                )}
                              </li>
                            );
                          })}
                        </ul>
                      )}
                    </li>
                  );
                })}
                {/* Columns whose table row was not itself a match (e.g. `col:email`) */}
                {filtering && [...db.columns.entries()]
                  .filter(([t]) => !db.groups.some((g) => g.items.some((n) => n.name === t)))
                  .map(([t, cols]) => (
                    <li key={`c/${t}`}>
                      <div className="row-item group">▾ {t}<span className="badge">{cols.length}</span></div>
                      <ul>
                        {cols.map((c) => (
                          <li key={c.name} className="row-item col"><Highlight text={c.name} terms={props.terms} /></li>
                        ))}
                      </ul>
                    </li>
                  ))}
              </ul>
            )}
          </li>
        );
      })}
    </ul>
  );
}
