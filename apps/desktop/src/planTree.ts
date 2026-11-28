/** Turns a server's query plan (MySQL's EXPLAIN FORMAT=JSON, or the EXPLAIN ANALYZE tree text) into a tree the window can draw. */
export interface PlanNode {
  id: number;
  label: string;
  detail: string;
  rows: number | null;
  /** The server's cost estimate. */
  cost: number | null;
  /** Measured time of this step on its own, in ms (only from EXPLAIN ANALYZE). */
  self: number | null;
  /** Measured time including everything below it. */
  total: number | null;
  loops: number | null;
  warn: string | null;
  children: PlanNode[];
}

let next = 1;
const node = (label: string, extra: Partial<PlanNode> = {}): PlanNode => ({ id: next++, label, detail: "", rows: null, cost: null, self: null, total: null, loops: null, warn: null, children: [], ...extra });
const num = (v: unknown): number | null => { const n = typeof v === "string" ? parseFloat(v) : typeof v === "number" ? v : NaN; return Number.isFinite(n) ? n : null; };

/** `EXPLAIN ANALYZE` text:  `-> Filter: (x)  (cost=1 rows=2) (actual time=0.1..3 rows=2 loops=1)`, children indented deeper. */
export function fromTimedTree(text: string): PlanNode[] {
  next = 1;
  const roots: PlanNode[] = [];
  const stack: { indent: number; n: PlanNode }[] = [];
  for (const raw of text.split("\n")) {
    const m = /^(\s*)-> (.*)$/.exec(raw);
    if (!m) continue;
    const indent = m[1].length;
    let body = m[2];
    const never = /\(never executed\)/.test(body);
    const actual = /\(actual time=([\d.e+-]+)\.\.([\d.e+-]+) rows=([\d.e+-]+) loops=(\d+)\)/.exec(body);
    const est = /\(cost=([\d.e+-]+) rows=([\d.e+-]+)\)/.exec(body);
    body = body.replace(/\s*\((cost=[^)]*|actual time=[^)]*|never executed)\)/g, "").trim();
    const [label, ...rest] = body.split(/:\s(.*)/s);
    const loops = actual ? Number(actual[4]) : null;
    const total = actual ? parseFloat(actual[2]) * (loops ?? 1) : null;
    const n = node(label.trim(), { detail: rest.join(" ").trim(), rows: actual ? num(actual[3]) : est ? num(est[2]) : null, cost: est ? num(est[1]) : null, total, loops, warn: never ? "never executed" : null });
    while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
    (stack.length ? stack[stack.length - 1].n.children : roots).push(n);
    stack.push({ indent, n });
  }
  const own = (n: PlanNode) => { n.children.forEach(own); if (n.total !== null) n.self = Math.max(0, n.total - n.children.reduce((s, c) => s + (c.total ?? 0), 0)); };
  roots.forEach(own);
  return roots;
}

const OPS: Record<string, string> = {
  ordering_operation: "Sort", grouping_operation: "Group", duplicates_removal: "Remove duplicates", buffer_result: "Buffer result",
  windowing: "Window", materialized_from_subquery: "Materialized subquery", union_result: "Union",
};
const LISTS = ["attached_subqueries", "select_list_subqueries", "optimized_away_subqueries", "query_specifications"];

function tableNode(t: Record<string, unknown>): PlanNode {
  const access = String(t.access_type ?? "");
  const key = t.key ? `key ${t.key}` : access === "ALL" ? "no index" : "";
  const cost = t.cost_info as Record<string, unknown> | undefined;
  const flags = [t.using_filesort ? "filesort" : "", t.using_temporary_table ? "temporary table" : ""].filter(Boolean).join(", ");
  return node(`${access === "ALL" ? "Full scan" : access ? `Lookup (${access})` : "Table"} on ${t.table_name ?? "?"}`, {
    detail: [key, t.attached_condition ? `where ${t.attached_condition}` : "", t.filtered ? `${t.filtered}% kept` : ""].filter(Boolean).join(" · "),
    rows: num(t.rows_examined_per_scan), cost: num(cost?.prefix_cost) ?? num(cost?.read_cost),
    warn: access === "ALL" ? "reads every row" : flags || null,
  });
}

function fromBlock(obj: Record<string, unknown>, label: string): PlanNode {
  const cost = obj.cost_info as Record<string, unknown> | undefined;
  const n = node(label, { cost: num(cost?.query_cost), warn: [obj.using_filesort ? "filesort" : "", obj.using_temporary_table ? "temporary table" : ""].filter(Boolean).join(", ") || null });
  for (const [k, v] of Object.entries(obj)) {
    if (k === "table" && v && typeof v === "object") n.children.push(tableNode(v as Record<string, unknown>));
    else if (k === "nested_loop" && Array.isArray(v)) {
      const j = node("Nested loop join");
      for (const item of v) if (item && typeof item === "object" && (item as Record<string, unknown>).table) j.children.push(tableNode((item as Record<string, Record<string, unknown>>).table));
      n.children.push(j);
    } else if (OPS[k] && v && typeof v === "object") n.children.push(fromBlock(v as Record<string, unknown>, OPS[k]));
    else if (k === "query_block" && v && typeof v === "object") n.children.push(fromBlock(v as Record<string, unknown>, "Query block"));
    else if (LISTS.includes(k) && Array.isArray(v)) for (const item of v) if (item && typeof item === "object") {
      const qb = (item as Record<string, unknown>).query_block;
      if (qb && typeof qb === "object") n.children.push(fromBlock(qb as Record<string, unknown>, k === "query_specifications" ? "Part of the union" : "Subquery"));
    }
  }
  return n;
}

export function fromJson(doc: unknown): PlanNode[] {
  next = 1;
  const qb = (doc as { query_block?: Record<string, unknown> } | null)?.query_block;
  return qb ? [fromBlock(qb, "Query")] : [];
}

/** PostgreSQL's `EXPLAIN (FORMAT JSON)`: one document with a `Plan` whose `Plans` are its steps. */
export function fromPostgres(doc: unknown): PlanNode[] {
  next = 1;
  const read = (p: Record<string, unknown>): PlanNode => {
    const loops = num(p["Actual Loops"]);
    const time = num(p["Actual Total Time"]);
    const kind = String(p["Node Type"] ?? "Step");
    const conditions = ["Index Name", "Index Cond", "Hash Cond", "Merge Cond", "Join Filter", "Filter"].filter((k) => p[k] !== undefined).map((k) => (k === "Index Name" ? `index ${p[k]}` : `${k.toLowerCase()} ${p[k]}`));
    const rows = num(p["Actual Rows"]) ?? num(p["Plan Rows"]);
    const n = node(p["Relation Name"] ? `${kind} on ${p["Relation Name"]}` : kind, {
      detail: conditions.join(" · "), rows, cost: num(p["Total Cost"]), loops,
      total: time === null ? null : time * (loops ?? 1),
      warn: loops === 0 ? "never executed" : kind === "Seq Scan" && (rows ?? 0) >= 1000 ? "reads every row" : p["Sort Space Type"] === "Disk" ? "sorted on disk" : null,
    });
    n.children = (Array.isArray(p.Plans) ? p.Plans : []).map((c) => read(c as Record<string, unknown>));
    if (n.total !== null) n.self = Math.max(0, n.total - n.children.reduce((s, c) => s + (c.total ?? 0), 0));
    return n;
  };
  const plan = Array.isArray(doc) ? (doc[0] as { Plan?: Record<string, unknown> } | undefined)?.Plan : undefined;
  return plan ? [read(plan)] : [];
}

/** All nodes, parents first. */
export function flatten(roots: PlanNode[]): PlanNode[] { return roots.flatMap((r) => [r, ...flatten(r.children)]); }

/** The step to look at first: the one that took longest on its own (measured), else the one with the highest cost. */
export function slowest(roots: PlanNode[]): PlanNode | null {
  const all = flatten(roots);
  const timed = all.filter((n) => n.self !== null && n.self > 0);
  if (timed.length) return timed.reduce((a, b) => ((b.self ?? 0) > (a.self ?? 0) ? b : a));
  // no measurements: the leaf the server expects to cost most (or to read most rows, when it gives no cost per step)
  const leaves = all.filter((n) => n.children.length === 0 && (n.cost !== null || n.rows !== null));
  const score = (n: PlanNode) => n.cost ?? n.rows ?? 0;
  return leaves.length ? leaves.reduce((a, b) => (score(b) > score(a) ? b : a)) : null;
}

/** One number to compare two runs: total measured time when there is one, else the estimated cost. */
export function summary(roots: PlanNode[]): { value: number; unit: "ms" | "cost" } | null {
  const t = roots.map((r) => r.total).filter((x): x is number => x !== null);
  if (t.length) return { value: Math.max(...t), unit: "ms" };
  const c = roots.map((r) => r.cost).filter((x): x is number => x !== null);
  return c.length ? { value: Math.max(...c), unit: "cost" } : null;
}
