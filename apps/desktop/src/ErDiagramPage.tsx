import { useEffect, useMemo, useRef, useState } from "react";
import Icon from "./Icon";
import { describeError, erApi, type ErModel, type ErRelation } from "./api";

const W = 220, HEAD = 26, ROW = 19, GAP_Y = 26, GAP_X = 130, PAD = 24, MAX_ROWS = 14;

interface Box { name: string; cols: { name: string; type: string; pk: boolean; fk: boolean }[]; more: number; h: number }

/** Depth of every table along its foreign keys: 0 for tables that reference nothing (the "parents"), one more than its deepest parent otherwise. */
export function depths(tables: string[], relations: { table: string; ref_table: string; ref_schema: string }[], db: string): Map<string, number> {
  const parents = new Map<string, string[]>(tables.map((t) => [t, []]));
  for (const r of relations) if (r.ref_schema === db && r.ref_table !== r.table && parents.has(r.table) && parents.has(r.ref_table)) parents.get(r.table)!.push(r.ref_table);
  const depth = new Map<string, number>(tables.map((t) => [t, 0]));
  // relax until stable; a cycle would grow forever, so the number of rounds is capped by the table count
  for (let round = 0; round < tables.length; round++) {
    let changed = false;
    for (const t of tables) for (const p of parents.get(t)!) if (depth.get(t)! < depth.get(p)! + 1 && depth.get(p)! + 1 <= tables.length) { depth.set(t, depth.get(p)! + 1); changed = true; }
    if (!changed) break;
  }
  return depth;
}

/** The model as Mermaid `erDiagram` text, to paste into documentation. */
export function toMermaid(model: ErModel, db: string, names: Set<string>): string {
  const id = (s: string) => s.replace(/[^A-Za-z0-9_]/g, "_");
  const lines = ["erDiagram"];
  for (const t of model.tables.filter((t) => names.has(t.name))) {
    lines.push(`  ${id(t.name)} {`);
    for (const c of t.columns) lines.push(`    ${id(c.column_type.split("(")[0].split(" ")[0] || "text")} ${id(c.name)}${c.primary ? " PK" : ""}`);
    lines.push("  }");
  }
  for (const r of model.relations.filter((r) => r.ref_schema === db && names.has(r.table) && names.has(r.ref_table))) lines.push(`  ${id(r.ref_table)} ||--o{ ${id(r.table)} : "${r.name.replace(/"/g, "'")}"`);
  return lines.join("\n");
}

/** Whole-database ER diagram: tables as boxes, foreign keys as lines, parents on the right and children on the left. Drag boxes, zoom, filter. */
export default function ErDiagramPage(props: { connectionId: number; database: string; onOpenTable(db: string, table: string): void }) {
  const [model, setModel] = useState<ErModel | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [onlyLinked, setOnlyLinked] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [moved, setMoved] = useState<Record<string, { x: number; y: number }>>({});
  const [copied, setCopied] = useState(false);

  const load = () => { setError(null); erApi.model(props.connectionId, props.database).then((m) => { setModel(m); setMoved({}); }).catch((e) => setError(describeError(e))); };
  useEffect(load, [props.connectionId, props.database]); // eslint-disable-line react-hooks/exhaustive-deps

  const { boxes, edges, pos, width, height, shown, total } = useMemo(() => {
    if (!model) return { boxes: [] as Box[], edges: [] as ErRelation[], pos: {} as Record<string, { x: number; y: number }>, width: 0, height: 0, shown: 0, total: 0 };
    const linked = new Set<string>();
    for (const r of model.relations) if (r.ref_schema === props.database) { linked.add(r.table); linked.add(r.ref_table); }
    const needle = filter.trim().toLowerCase();
    const keep = model.tables.filter((t) => (!needle || t.name.toLowerCase().includes(needle)) && (!onlyLinked || linked.has(t.name)));
    const names = new Set(keep.map((t) => t.name));
    const fkCols = new Map<string, Set<string>>();
    for (const r of model.relations) fkCols.set(r.table, new Set([...(fkCols.get(r.table) ?? []), ...r.columns]));
    const boxes: Box[] = keep.map((t) => {
      const cols = t.columns.slice(0, MAX_ROWS).map((c) => ({ name: c.name, type: c.column_type, pk: c.primary, fk: fkCols.get(t.name)?.has(c.name) ?? false }));
      const more = Math.max(0, t.columns.length - MAX_ROWS);
      return { name: t.name, cols, more, h: HEAD + (cols.length + (more ? 1 : 0)) * ROW + 6 };
    });
    const edges = model.relations.filter((r) => r.ref_schema === props.database && names.has(r.table) && names.has(r.ref_table));
    const depth = depths(keep.map((t) => t.name), edges, props.database);
    // columns of boxes by depth: the deepest children on the left, the parents on the right; tables with no relation go in a grid underneath
    const connected = boxes.filter((b) => edges.some((e) => e.table === b.name || e.ref_table === b.name));
    const loose = boxes.filter((b) => !connected.includes(b));
    const maxDepth = Math.max(0, ...connected.map((b) => depth.get(b.name)!));
    const pos: Record<string, { x: number; y: number }> = {};
    const heights: number[] = [];
    for (let d = maxDepth; d >= 0; d--) {
      const col = maxDepth - d;
      let y = PAD;
      for (const b of connected.filter((b) => depth.get(b.name) === d)) { pos[b.name] = { x: PAD + col * (W + GAP_X), y }; y += b.h + GAP_Y; }
      heights.push(y);
    }
    const topH = Math.max(PAD, ...heights);
    const perRow = Math.max(1, Math.floor((Math.max(connected.length ? (maxDepth + 1) * (W + GAP_X) : 4 * (W + GAP_X), 3 * (W + GAP_X))) / (W + 40)));
    let y = topH + (loose.length && connected.length ? 30 : 0), x = 0, rowH = 0;
    for (const b of loose) {
      if (x >= perRow) { x = 0; y += rowH + GAP_Y; rowH = 0; }
      pos[b.name] = { x: PAD + x * (W + 40), y }; rowH = Math.max(rowH, b.h); x++;
    }
    const width = Math.max(...Object.values(pos).map((p) => p.x + W), 0) + PAD;
    const height = Math.max(...boxes.map((b) => pos[b.name].y + b.h), 0) + PAD;
    return { boxes, edges, pos, width, height, shown: keep.length, total: model.tables.length };
  }, [model, filter, onlyLinked, props.database]);

  const at = (n: string) => moved[n] ?? pos[n] ?? { x: 0, y: 0 };
  const bounds = useMemo(() => {
    let w = width, h = height;
    for (const b of boxes) { const p = moved[b.name] ?? pos[b.name]; if (p) { w = Math.max(w, p.x + W + PAD); h = Math.max(h, p.y + b.h + PAD); } }
    return { w, h };
  }, [boxes, pos, moved, width, height]);

  const rowY = (table: string, col: string) => {
    const b = boxes.find((x) => x.name === table), p = at(table), i = b ? b.cols.findIndex((c) => c.name === col) : -1;
    return p.y + (i < 0 ? HEAD / 2 : HEAD + i * ROW + ROW / 2);
  };
  const path = (e: ErRelation) => {
    const a = at(e.table), b = at(e.ref_table), y1 = rowY(e.table, e.columns[0]), y2 = rowY(e.ref_table, e.ref_columns[0]);
    if (e.table === e.ref_table) return `M ${a.x + W} ${y1} C ${a.x + W + 60} ${y1}, ${a.x + W + 60} ${y2}, ${a.x + W} ${y2}`;
    if (a.x + W + 20 <= b.x) { const x1 = a.x + W, x2 = b.x, dx = Math.max(40, (x2 - x1) / 2); return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`; }
    if (b.x + W + 20 <= a.x) { const x1 = a.x, x2 = b.x + W, dx = Math.max(40, (x1 - x2) / 2); return `M ${x1} ${y1} C ${x1 - dx} ${y1}, ${x2 + dx} ${y2}, ${x2} ${y2}`; }
    const x1 = a.x + W, x2 = b.x + W; return `M ${x1} ${y1} C ${x1 + 60} ${y1}, ${x2 + 60} ${y2}, ${x2} ${y2}`;
  };

  const drag = useRef<{ n: string; sx: number; sy: number; ox: number; oy: number } | null>(null);
  const startDrag = (e: React.PointerEvent, n: string) => {
    if (e.button !== 0) return;
    const p = at(n); drag.current = { n, sx: e.clientX, sy: e.clientY, ox: p.x, oy: p.y };
    const move = (ev: PointerEvent) => { const d = drag.current; if (d) setMoved((m) => ({ ...m, [d.n]: { x: Math.max(0, d.ox + (ev.clientX - d.sx) / zoom), y: Math.max(0, d.oy + (ev.clientY - d.sy) / zoom) } })); };
    const up = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", up); drag.current = null; };
    window.addEventListener("pointermove", move); window.addEventListener("pointerup", up);
  };

  if (!model) return <div className="diagram"><div className="empty">{error ? <span className="error">{error}</span> : "Loading…"}</div></div>;
  return (
    <div className="diagram">
      <div className="dgm-bar">
        <input aria-label="Filter tables" placeholder="Filter tables" value={filter} onChange={(e) => setFilter(e.target.value)} style={{ width: 160 }} />
        <label className="check"><input type="checkbox" checked={onlyLinked} onChange={(e) => setOnlyLinked(e.target.checked)} /> only tables with relations</label>
        <button onClick={load} title="Read the schema again"><Icon name="refresh" /> Refresh</button>
        <button onClick={() => setMoved({})} disabled={Object.keys(moved).length === 0}>Reset layout</button>
        <button onClick={() => { navigator.clipboard.writeText(toMermaid(model, props.database, new Set(boxes.map((b) => b.name)))); setCopied(true); setTimeout(() => setCopied(false), 1500); }} title="Copy the diagram as Mermaid erDiagram text">{copied ? "Copied" : "Copy as Mermaid"}</button>
        <span className="sep" />
        <button aria-label="Zoom out" onClick={() => setZoom((z) => Math.max(0.3, +(z - 0.1).toFixed(2)))}>−</button>
        <button className="zoom" onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
        <button aria-label="Zoom in" onClick={() => setZoom((z) => Math.min(1.6, +(z + 0.1).toFixed(2)))}>+</button>
        <span className="grow" />
        <span className="dim" aria-label="Counts">{shown} of {total} tables · {edges.length} relation{edges.length === 1 ? "" : "s"}</span>
      </div>
      <div className="dgm-scroll">
        {total === 0 && <div className="dgm-note">This database has no tables.</div>}
        <div style={{ width: bounds.w * zoom, height: bounds.h * zoom }}>
          <div className="dgm-canvas" style={{ width: bounds.w, height: bounds.h, transform: `scale(${zoom})` }}>
            <svg className="dgm-edges" width={bounds.w} height={bounds.h}>
              <defs>
                <marker id="er-dot" viewBox="0 0 8 8" refX="4" refY="4" markerWidth="8" markerHeight="8"><circle cx="4" cy="4" r="3" className="dgm-dot" /></marker>
                <marker id="er-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="9" markerHeight="9" orient="auto-start-reverse"><path d="M1 1 9 5 1 9z" className="dgm-arrow" /></marker>
              </defs>
              {edges.map((e) => (
                <g key={`${e.table}.${e.name}`} className="dgm-edge" data-edge={`${e.table}>${e.ref_table}`}>
                  <path d={path(e)} markerStart="url(#er-dot)" markerEnd="url(#er-arrow)" />
                  <path d={path(e)} className="hit"><title>{`${e.table}.${e.columns.join(", ")} → ${e.ref_table}.${e.ref_columns.join(", ")}\n${e.name}\nON DELETE ${e.on_delete || "—"} · ON UPDATE ${e.on_update || "—"}`}</title></path>
                </g>
              ))}
            </svg>
            {boxes.map((b) => {
              const p = at(b.name);
              return (
                <div key={b.name} className="dgm-box" data-table={b.name} style={{ left: p.x, top: p.y, width: W }}>
                  <div className="dgm-head" onPointerDown={(e) => startDrag(e, b.name)} onDoubleClick={() => props.onOpenTable(props.database, b.name)} title="Drag to move, double-click to open this table">
                    <Icon name="table" size={12} /> <span>{b.name}</span>
                  </div>
                  {b.cols.map((c) => (
                    <div key={c.name} className={`dgm-row ${c.fk ? "linked" : ""}`}>
                      <span className="key">{c.pk ? <Icon name="key" size={11} className="ico-pk" /> : c.fk ? <Icon name="link" size={11} /> : <Icon name="column" size={10} className="ico-col" />}</span>
                      <span className={`name ${c.pk ? "pk" : ""}`}>{c.name}</span>
                      <span className="ctype">{c.type}</span>
                    </div>
                  ))}
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
