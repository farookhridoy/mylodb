import { useEffect, useMemo, useRef, useState } from "react";
import Icon from "./Icon";
import { api, type ColumnDetail, type ForeignKeyInfo } from "./api";
import { useTableOverview } from "./tableOverview";

const W = 230, HEAD = 26, ROW = 20, GAP_Y = 28, GAP_X = 150, PAD = 28, MAX_ROWS = 30, MAX_PER_SIDE = 12;

interface BoxCol { name: string; type: string; pk: boolean; linked: boolean }
interface Box { id: string; db: string; name: string; cols: BoxCol[]; more: number; center: boolean }
interface Edge { key: string; from: string; fromCol: string; to: string; toCol: string; fk: ForeignKeyInfo }

const idOf = (db: string, table: string) => `${db}.${table}`;
const boxHeight = (b: Box) => HEAD + (b.cols.length + (b.more ? 1 : 0)) * ROW + 6;

/** Table "Diagram" view: this table with the tables it references (right) and the tables that reference it (left). */
export default function TableDiagram(props: { connectionId: number; db: string; table: string; onOpenTable(db: string, table: string): void }) {
  const { data: o, loading, error, refresh } = useTableOverview(props.connectionId, props.db, props.table);
  const [related, setRelated] = useState<Record<string, ColumnDetail[]>>({});
  const [moved, setMoved] = useState<Record<string, { x: number; y: number }>>({});
  const [zoom, setZoom] = useState(1);
  const selfId = idOf(props.db, props.table);

  // the neighbours: referenced tables (parents) and referencing tables (children), without duplicates, capped per side
  const { parents, children, edges, hidden } = useMemo(() => {
    const parents = new Map<string, { db: string; name: string }>();
    const children = new Map<string, { db: string; name: string }>();
    const edges: Edge[] = [];
    const seen = new Set<string>();
    let hidden = 0;
    const add = (f: ForeignKeyInfo) => {
      const k = `${f.schema}.${f.table}.${f.name}`;
      if (seen.has(k)) return;
      seen.add(k);
      const from = idOf(f.schema, f.table), to = idOf(f.ref_schema, f.ref_table);
      for (const [id, db, name, side] of [[from, f.schema, f.table, children], [to, f.ref_schema, f.ref_table, parents]] as const) {
        if (id === selfId || parents.has(id) || children.has(id)) continue;
        if (side.size >= MAX_PER_SIDE) { hidden++; return; }
        side.set(id, { db, name });
      }
      edges.push({ key: k, from, fromCol: f.columns[0], to, toCol: f.ref_columns[0], fk: f });
    };
    o?.outgoing.forEach(add);
    o?.incoming.forEach(add);
    return { parents, children, edges, hidden };
  }, [o, selfId]);

  // the neighbours' columns
  useEffect(() => {
    let live = true;
    setMoved({});
    const all = [...parents.values(), ...children.values()];
    Promise.all(all.map((t) => api.columnDetails(props.connectionId, t.db, t.name).then((c) => [idOf(t.db, t.name), c] as const).catch(() => [idOf(t.db, t.name), []] as const)))
      .then((pairs) => live && setRelated(Object.fromEntries(pairs)));
    return () => { live = false; };
  }, [parents, children, props.connectionId]);

  const boxes: Box[] = useMemo(() => {
    if (!o) return [];
    const linked = new Map<string, Set<string>>();
    const mark = (id: string, c: string) => linked.set(id, (linked.get(id) ?? new Set()).add(c));
    edges.forEach((e) => { mark(e.from, e.fromCol); mark(e.to, e.toCol); });
    const make = (id: string, db: string, name: string, cols: { name: string; type: string; pk: boolean }[], center: boolean): Box => ({
      id, db, name, center, more: Math.max(0, cols.length - MAX_ROWS),
      cols: cols.slice(0, MAX_ROWS).map((c) => ({ ...c, linked: linked.get(id)?.has(c.name) ?? false })),
    });
    const self = make(selfId, props.db, props.table, o.columns.map((c) => ({ name: c.name, type: c.column_type, pk: c.key === "PRI" })), true);
    const side = (m: Map<string, { db: string; name: string }>) =>
      [...m.entries()].map(([id, t]) => make(id, t.db, t.name, (related[id] ?? []).map((c) => ({ name: c.name, type: c.column_type, pk: c.key === "PRI" })), false));
    return [...side(children), self, ...side(parents)];
  }, [o, edges, related, children, parents, props.db, props.table, selfId]);

  // layout: children on the left, this table in the middle, parents on the right
  const layout = useMemo(() => {
    const pos: Record<string, { x: number; y: number }> = {};
    const stack = (list: Box[], x: number) => { let y = PAD; for (const b of list) { pos[b.id] = { x, y }; y += boxHeight(b) + GAP_Y; } return y - GAP_Y + PAD; };
    const left = boxes.filter((b) => children.has(b.id)), right = boxes.filter((b) => parents.has(b.id)), self = boxes.find((b) => b.center);
    const hasLeft = left.length > 0;
    const hLeft = stack(left, PAD);
    const cx = PAD + (hasLeft ? W + GAP_X : 0);
    if (self) pos[self.id] = { x: cx, y: PAD };
    const hRight = stack(right, cx + W + GAP_X + (edges.some((e) => e.from === e.to) ? 40 : 0));
    const height = Math.max(hLeft, hRight, self ? boxHeight(self) + 2 * PAD : 0);
    const width = (right.length ? (pos[right[0].id]?.x ?? 0) + W : cx + W + (edges.some((e) => e.from === e.to) ? 80 : 0)) + PAD;
    return { pos, width, height };
  }, [boxes, children, parents, edges]);

  const at = (id: string) => moved[id] ?? layout.pos[id] ?? { x: 0, y: 0 };
  const bounds = useMemo(() => {
    let w = layout.width, h = layout.height;
    for (const b of boxes) { const p = moved[b.id] ?? layout.pos[b.id]; if (p) { w = Math.max(w, p.x + W + PAD); h = Math.max(h, p.y + boxHeight(b) + PAD); } }
    return { w, h };
  }, [boxes, layout, moved]);

  const rowY = (id: string, col: string) => {
    const b = boxes.find((x) => x.id === id), p = at(id);
    const i = b ? b.cols.findIndex((c) => c.name === col) : -1;
    return p.y + (i < 0 ? HEAD / 2 : HEAD + i * ROW + ROW / 2);
  };
  const edgePath = (e: Edge) => {
    const a = at(e.from), b = at(e.to);
    const y1 = rowY(e.from, e.fromCol), y2 = rowY(e.to, e.toCol);
    if (e.from === e.to) return { d: `M ${a.x + W} ${y1} C ${a.x + W + 70} ${y1}, ${a.x + W + 70} ${y2}, ${a.x + W} ${y2}`, mid: { x: a.x + W + 52, y: (y1 + y2) / 2 } };
    let x1: number, x2: number, c1: number, c2: number;
    if (a.x + W + 20 <= b.x) { x1 = a.x + W; x2 = b.x; const dx = Math.max(40, (x2 - x1) / 2); c1 = x1 + dx; c2 = x2 - dx; }
    else if (b.x + W + 20 <= a.x) { x1 = a.x; x2 = b.x + W; const dx = Math.max(40, (x1 - x2) / 2); c1 = x1 - dx; c2 = x2 + dx; }
    else { x1 = a.x + W; x2 = b.x + W; c1 = x1 + 60; c2 = x2 + 60; }
    return { d: `M ${x1} ${y1} C ${c1} ${y1}, ${c2} ${y2}, ${x2} ${y2}`, mid: { x: (x1 + x2) / 2, y: (y1 + y2) / 2 } };
  };

  // dragging a table box by its header
  const drag = useRef<{ id: string; sx: number; sy: number; ox: number; oy: number; moved: boolean } | null>(null);
  const startDrag = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0) return;
    const p = at(id);
    drag.current = { id, sx: e.clientX, sy: e.clientY, ox: p.x, oy: p.y, moved: false };
    const move = (ev: PointerEvent) => {
      const d = drag.current; if (!d) return;
      const dx = (ev.clientX - d.sx) / zoom, dy = (ev.clientY - d.sy) / zoom;
      if (Math.abs(dx) + Math.abs(dy) > 3) d.moved = true;
      setMoved((m) => ({ ...m, [d.id]: { x: Math.max(0, d.ox + dx), y: Math.max(0, d.oy + dy) } }));
    };
    const up = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); drag.current = null; };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  if (!o) return <div className="diagram"><div className="empty">{error ? <span className="error">{error}</span> : "Loading…"}</div></div>;
  if (o.columns.length === 0) return <div className="diagram"><div className="empty">{props.db}.{props.table} was not found.</div></div>;

  return (
    <div className="diagram">
      <div className="dgm-bar">
        <button onClick={refresh} disabled={loading} title="Read the relations again"><Icon name="refresh" /> Refresh</button>
        <button onClick={() => setMoved({})} disabled={Object.keys(moved).length === 0} title="Put the tables back where they were placed automatically">Reset layout</button>
        <span className="sep" />
        <button aria-label="Zoom out" onClick={() => setZoom((z) => Math.max(0.4, +(z - 0.1).toFixed(2)))}>−</button>
        <button className="zoom" onClick={() => setZoom(1)} title="Reset zoom">{Math.round(zoom * 100)}%</button>
        <button aria-label="Zoom in" onClick={() => setZoom((z) => Math.min(1.6, +(z + 0.1).toFixed(2)))}>+</button>
        <span className="grow" />
        <span className="dim">{boxes.length} {boxes.length === 1 ? "table" : "tables"} · {edges.length} {edges.length === 1 ? "relation" : "relations"}{hidden > 0 ? ` · ${hidden} more not shown` : ""}</span>
      </div>
      <div className="dgm-scroll">
        {edges.length === 0 && <div className="dgm-note">This table has no foreign keys and no other table references it.</div>}
        <div style={{ width: bounds.w * zoom, height: bounds.h * zoom }}>
          <div className="dgm-canvas" style={{ width: bounds.w, height: bounds.h, transform: `scale(${zoom})` }}>
            <svg className="dgm-edges" width={bounds.w} height={bounds.h}>
              <defs>
                <marker id="dgm-dot" viewBox="0 0 8 8" refX="4" refY="4" markerWidth="8" markerHeight="8"><circle cx="4" cy="4" r="3" className="dgm-dot" /></marker>
                <marker id="dgm-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" orient="auto-start-reverse"><path d="M1 1 9 5 1 9z" className="dgm-arrow" /></marker>
              </defs>
              {edges.map((e) => {
                const { d, mid } = edgePath(e);
                const involvesSelf = e.from === selfId || e.to === selfId;
                return (
                  <g key={e.key} className={`dgm-edge ${involvesSelf ? "main" : ""}`}>
                    <path d={d} markerStart="url(#dgm-dot)" markerEnd="url(#dgm-arrow)" />
                    <path d={d} className="hit"><title>{`${e.fk.table}.${e.fk.columns.join(", ")} → ${e.fk.ref_table}.${e.fk.ref_columns.join(", ")}\n${e.fk.name}\nON DELETE ${e.fk.on_delete || "—"} · ON UPDATE ${e.fk.on_update || "—"}`}</title></path>
                    <text x={mid.x} y={mid.y - 4} textAnchor="middle" className="dgm-label">{e.fk.name.length > 26 ? e.fk.name.slice(0, 25) + "…" : e.fk.name}</text>
                  </g>
                );
              })}
            </svg>
            {boxes.map((b) => {
              const p = at(b.id);
              return (
                <div key={b.id} className={`dgm-box ${b.center ? "center" : ""}`} style={{ left: p.x, top: p.y, width: W }}>
                  <div className="dgm-head" onPointerDown={(e) => startDrag(e, b.id)} onDoubleClick={() => !b.center && props.onOpenTable(b.db, b.name)}
                       title={b.center ? "Drag to move" : "Drag to move, double-click to open this table"}>
                    <Icon name="table" size={12} /> <span>{b.name}</span>{b.db !== props.db && <span className="dim"> · {b.db}</span>}
                  </div>
                  {b.cols.map((c) => (
                    <div key={c.name} className={`dgm-row ${c.linked ? "linked" : ""}`}>
                      <span className="key">{c.pk ? <Icon name="key" size={11} className="ico-pk" /> : c.linked ? <Icon name="link" size={11} /> : <Icon name="column" size={10} className="ico-col" />}</span>
                      <span className={`name ${c.pk ? "pk" : ""}`}>{c.name}</span>
                      <span className="ctype">{c.type}</span>
                    </div>
                  ))}
                  {b.cols.length === 0 && <div className="dgm-row dim">loading columns…</div>}
                  {b.more > 0 && <div className="dgm-row dim">… {b.more} more columns</div>}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
