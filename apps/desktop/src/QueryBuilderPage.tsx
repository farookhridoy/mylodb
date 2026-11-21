import { useEffect, useMemo, useState } from "react";
import Select from "./Select";
import { api, builderApi, describeError, type BuilderOp, type ColumnDetail, type ForeignKeyInfo, type QuerySpec } from "./api";

interface JoinState { kind: "inner" | "left"; table: string; alias: string; on: { left: string; leftCol: string; rightCol: string }[] }
interface Pick { alias: string; column: string; agg: string; name: string }
interface FilterState { alias: string; column: string; op: BuilderOp; v1: string; v2: string }
interface OrderState { alias: string; column: string; desc: boolean }

const OPS: { op: BuilderOp; label: string; arity: 0 | 1 | 2 }[] = [
  { op: "eq", label: "=", arity: 1 }, { op: "ne", label: "≠", arity: 1 }, { op: "lt", label: "<", arity: 1 }, { op: "le", label: "≤", arity: 1 }, { op: "gt", label: ">", arity: 1 }, { op: "ge", label: "≥", arity: 1 },
  { op: "contains", label: "contains", arity: 1 }, { op: "starts_with", label: "starts with", arity: 1 }, { op: "ends_with", label: "ends with", arity: 1 },
  { op: "in", label: "in (a, b, …)", arity: 1 }, { op: "between", label: "between", arity: 2 }, { op: "is_null", label: "is NULL", arity: 0 }, { op: "not_null", label: "is not NULL", arity: 0 },
];
const NUMERIC = /int|decimal|numeric|float|double|real|bit|year/i;

/** Pick a short, unique alias for a table: its initials, then a number. */
const aliasFor = (table: string, taken: string[]) => {
  const base = (table.match(/[a-z]/gi) ? table.split(/[^a-z0-9]+/i).filter(Boolean).map((p) => p[0]).join("") : "t").toLowerCase() || "t";
  let a = base, n = 2;
  while (taken.includes(a)) a = `${base}${n++}`;
  return a;
};

/**
 * Visual query builder: choose tables, join them, pick columns, add conditions, and read the SQL it makes. The statement is built by the
 * backend (`myq_schema::builder`, with the connection's quoting); "Open in editor" puts it in a query tab, "Run" also runs it.
 */
export default function QueryBuilderPage(props: { connectionId: number; database: string; tables: string[]; onUse(sql: string, run: boolean): void }) {
  const [base, setBase] = useState<string>("");
  const [joins, setJoins] = useState<JoinState[]>([]);
  const [picks, setPicks] = useState<Pick[]>([]);
  const [filters, setFilters] = useState<FilterState[]>([]);
  const [order, setOrder] = useState<OrderState[]>([]);
  const [limit, setLimit] = useState("100");
  const [distinct, setDistinct] = useState(false);
  const [cols, setCols] = useState<Record<string, ColumnDetail[]>>({});
  const [fks, setFks] = useState<Record<string, { outgoing: ForeignKeyInfo[]; incoming: ForeignKeyInfo[] }>>({});
  const [sql, setSql] = useState("");
  const [problem, setProblem] = useState<string | null>(null);

  const baseAlias = base ? aliasFor(base, []) : "";
  const members = useMemo(() => (base ? [{ alias: baseAlias, table: base }, ...joins.map((j) => ({ alias: j.alias, table: j.table }))] : []), [base, baseAlias, joins]);

  const need = (table: string) => {
    if (cols[table]) return;
    api.columnDetails(props.connectionId, props.database, table).then((c) => setCols((m) => ({ ...m, [table]: c }))).catch(() => setCols((m) => ({ ...m, [table]: [] })));
    api.tableOverview(props.connectionId, props.database, table).then((o) => setFks((m) => ({ ...m, [table]: { outgoing: o.outgoing, incoming: o.incoming } }))).catch(() => {});
  };
  useEffect(() => { members.forEach((m) => need(m.table)); }, [members]); // eslint-disable-line react-hooks/exhaustive-deps

  const chooseBase = (t: string) => { setBase(t); setJoins([]); setPicks([]); setFilters([]); setOrder([]); };

  /** A join to `table`, with the ON pairs taken from a foreign key between it and a table already in the query when there is one. */
  const addJoin = (table: string) => {
    const taken = members.map((m) => m.alias);
    const alias = aliasFor(table, taken);
    let on: JoinState["on"] = [];
    for (const m of members) {
      const f = fks[m.table];
      const out = f?.outgoing.find((x) => x.ref_table === table && (x.ref_schema === props.database));
      const inc = f?.incoming.find((x) => x.table === table && x.schema === props.database);
      if (out) { on = out.columns.map((c, i) => ({ left: m.alias, leftCol: c, rightCol: out.ref_columns[i] })); break; }
      if (inc) { on = inc.ref_columns.map((c, i) => ({ left: m.alias, leftCol: c, rightCol: inc.columns[i] })); break; }
    }
    setJoins([...joins, { kind: "inner", table, alias, on: on.length ? on : [{ left: members[0].alias, leftCol: "", rightCol: "" }] }]);
    need(table);
  };

  const spec: QuerySpec | null = useMemo(() => {
    if (!base) return null;
    const colType = (alias: string, column: string) => cols[members.find((m) => m.alias === alias)?.table ?? ""]?.find((c) => c.name === column)?.column_type ?? "";
    return {
      from: { db: props.database, table: base, alias: baseAlias },
      joins: joins.map((j) => ({ kind: j.kind, table: { db: props.database, table: j.table, alias: j.alias }, on: j.on.map((o) => [{ alias: o.left, column: o.leftCol }, o.rightCol] as [{ alias: string; column: string }, string]) })),
      select: picks.map((p) => ({ alias: p.alias, column: p.column, name: p.name || null, agg: (p.agg || null) as QuerySpec["select"][number]["agg"] })),
      distinct,
      filters: filters.map((f) => {
        const info = OPS.find((o) => o.op === f.op)!;
        return { alias: f.alias, column: f.column, op: f.op, values: info.arity === 0 ? [] : info.arity === 2 ? [f.v1, f.v2] : [f.v1], numeric: NUMERIC.test(colType(f.alias, f.column)) };
      }),
      order: order.map((o) => ({ alias: o.alias, column: o.column, desc: o.desc })),
      limit: /^\d+$/.test(limit.trim()) ? Number(limit) : null,
    };
  }, [base, baseAlias, joins, picks, filters, order, limit, distinct, cols, members, props.database]);

  useEffect(() => {
    if (!spec) { setSql(""); setProblem(null); return; }
    let live = true;
    const t = setTimeout(() => builderApi.build(props.connectionId, spec).then((s) => { if (live) { setSql(s); setProblem(null); } }).catch((e) => { if (live) { setSql(""); setProblem(describeError(e)); } }), 150);
    return () => { live = false; clearTimeout(t); };
  }, [spec, props.connectionId]);

  const columnsOf = (alias: string) => cols[members.find((m) => m.alias === alias)?.table ?? ""] ?? [];
  const aliasSelect = (value: string, onChange: (a: string) => void, label: string) => (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>{members.map((m) => <option key={m.alias} value={m.alias}>{m.table} ({m.alias})</option>)}</select>
  );
  const colSelect = (alias: string, value: string, onChange: (c: string) => void, label: string, withStar = false) => (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="">column…</option>
      {withStar && <option value="*">* (all)</option>}
      {columnsOf(alias).map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
    </select>
  );
  const first = members[0]?.alias ?? "";
  const set = <T,>(list: T[], i: number, patch: Partial<T>, apply: (l: T[]) => void) => apply(list.map((x, k) => (k === i ? { ...x, ...patch } : x)));
  const del = <T,>(list: T[], i: number, apply: (l: T[]) => void) => apply(list.filter((_, k) => k !== i));

  return (
    <div className="admin-page qb">
      <h2>Query builder <span className="dim">{props.database}</span></h2>
      <section>
        <h3>Tables</h3>
        <label>From
          <Select value={base} onChange={(e) => chooseBase(e.target.value)}>
            <option value="">Choose a table…</option>
            {props.tables.map((t) => <option key={t} value={t}>{t}</option>)}
          </Select>
        </label>
        {joins.map((j, i) => (
          <div className="qb-row" key={j.alias}>
            <select aria-label={`Join type ${j.alias}`} value={j.kind} onChange={(e) => set(joins, i, { kind: e.target.value as "inner" | "left" }, setJoins)}><option value="inner">INNER JOIN</option><option value="left">LEFT JOIN</option></select>
            <span className="mono">{j.table} ({j.alias})</span> on
            {j.on.map((o, k) => (
              <span key={k} className="qb-on">
                {aliasSelect(o.left, (a) => set(joins, i, { on: j.on.map((x, n) => (n === k ? { ...x, left: a, leftCol: "" } : x)) }, setJoins), `Join ${j.alias} left table`)}
                {colSelect(o.left, o.leftCol, (c) => set(joins, i, { on: j.on.map((x, n) => (n === k ? { ...x, leftCol: c } : x)) }, setJoins), `Join ${j.alias} left column`)}
                =
                <select aria-label={`Join ${j.alias} right column`} value={o.rightCol} onChange={(e) => set(joins, i, { on: j.on.map((x, n) => (n === k ? { ...x, rightCol: e.target.value } : x)) }, setJoins)}>
                  <option value="">column…</option>{(cols[j.table] ?? []).map((c) => <option key={c.name} value={c.name}>{c.name}</option>)}
                </select>
              </span>
            ))}
            <button className="link" onClick={() => { setJoins(joins.filter((_, k) => k !== i)); setPicks(picks.filter((p) => p.alias !== j.alias)); setFilters(filters.filter((f) => f.alias !== j.alias)); setOrder(order.filter((o) => o.alias !== j.alias)); }}>remove</button>
          </div>
        ))}
        {base && (
          <label>Join another table
            <Select value="" onChange={(e) => e.target.value && addJoin(e.target.value)}>
              <option value="">Add a join…</option>
              {props.tables.map((t) => <option key={t} value={t}>{t}</option>)}
            </Select>
          </label>
        )}
      </section>
      {base && (
        <>
          <section>
            <h3>Columns <span className="dim small">(none chosen = every column)</span></h3>
            {picks.map((p, i) => (
              <div className="qb-row" key={i}>
                {aliasSelect(p.alias, (a) => set(picks, i, { alias: a, column: "" }, setPicks), `Column ${i + 1} table`)}
                {colSelect(p.alias, p.column, (c) => set(picks, i, { column: c }, setPicks), `Column ${i + 1}`, true)}
                <select aria-label={`Column ${i + 1} function`} value={p.agg} onChange={(e) => set(picks, i, { agg: e.target.value }, setPicks)}>
                  <option value="">as is</option><option value="count">COUNT</option><option value="sum">SUM</option><option value="avg">AVG</option><option value="min">MIN</option><option value="max">MAX</option>
                </select>
                <input aria-label={`Column ${i + 1} name`} placeholder="name (AS)" value={p.name} onChange={(e) => set(picks, i, { name: e.target.value }, setPicks)} />
                <button className="link" onClick={() => del(picks, i, setPicks)}>remove</button>
              </div>
            ))}
            <button onClick={() => setPicks([...picks, { alias: first, column: "", agg: "", name: "" }])}>+ Column</button>
            <label className="check"><input type="checkbox" checked={distinct} onChange={(e) => setDistinct(e.target.checked)} /> DISTINCT</label>
          </section>
          <section>
            <h3>Conditions <span className="dim small">(all must hold)</span></h3>
            {filters.map((f, i) => {
              const info = OPS.find((o) => o.op === f.op)!;
              return (
                <div className="qb-row" key={i}>
                  {aliasSelect(f.alias, (a) => set(filters, i, { alias: a, column: "" }, setFilters), `Condition ${i + 1} table`)}
                  {colSelect(f.alias, f.column, (c) => set(filters, i, { column: c }, setFilters), `Condition ${i + 1} column`)}
                  <select aria-label={`Condition ${i + 1} operator`} value={f.op} onChange={(e) => set(filters, i, { op: e.target.value as BuilderOp }, setFilters)}>{OPS.map((o) => <option key={o.op} value={o.op}>{o.label}</option>)}</select>
                  {info.arity >= 1 && <input aria-label={`Condition ${i + 1} value`} value={f.v1} onChange={(e) => set(filters, i, { v1: e.target.value }, setFilters)} />}
                  {info.arity === 2 && <><span>and</span><input aria-label={`Condition ${i + 1} second value`} value={f.v2} onChange={(e) => set(filters, i, { v2: e.target.value }, setFilters)} /></>}
                  <button className="link" onClick={() => del(filters, i, setFilters)}>remove</button>
                </div>
              );
            })}
            <button onClick={() => setFilters([...filters, { alias: first, column: "", op: "eq", v1: "", v2: "" }])}>+ Condition</button>
          </section>
          <section>
            <h3>Order and limit</h3>
            {order.map((o, i) => (
              <div className="qb-row" key={i}>
                {aliasSelect(o.alias, (a) => set(order, i, { alias: a, column: "" }, setOrder), `Order ${i + 1} table`)}
                {colSelect(o.alias, o.column, (c) => set(order, i, { column: c }, setOrder), `Order ${i + 1} column`)}
                <label className="check"><input type="checkbox" checked={o.desc} onChange={(e) => set(order, i, { desc: e.target.checked }, setOrder)} /> descending</label>
                <button className="link" onClick={() => del(order, i, setOrder)}>remove</button>
              </div>
            ))}
            <button onClick={() => setOrder([...order, { alias: first, column: "", desc: false }])}>+ Order by</button>
            <label>Limit <input aria-label="Limit" value={limit} onChange={(e) => setLimit(e.target.value)} style={{ width: 70 }} /></label>
          </section>
        </>
      )}
      <section>
        <h3>SQL</h3>
        {problem && <div className="banner bad" aria-label="Problem">{problem}</div>}
        <pre className="preview" aria-label="Generated SQL">{sql || (problem ? "" : "Choose a table to start.")}</pre>
        <div className="actions" style={{ justifyContent: "flex-start" }}>
          <button disabled={!sql} onClick={() => props.onUse(sql, false)}>Open in editor</button>
          <button className="primary" disabled={!sql} onClick={() => props.onUse(sql, true)}>Run</button>
        </div>
      </section>
    </div>
  );
}
