import { useMemo, useState, type ReactElement } from "react";
import Icon from "./Icon";
import type { ExplainResult, ProfileResult } from "./api";
import { Grid } from "./ResultsPane";
import { flatten, fromJson, fromTimedTree, slowest, summary, type PlanNode } from "./planTree";

export type Analysis =
  | { kind: "explain" | "analyze"; sql: string; running: boolean; error: string | null; data: ExplainResult | null; prev?: ExplainResult | null }
  | { kind: "profile"; sql: string; running: boolean; error: string | null; data: ProfileResult | null; prev?: ProfileResult | null };

const ICON = { warning: <Icon name="warn" size={13} />, info: <Icon name="info" size={13} />, good: <Icon name="check" size={13} /> } as const;

/** Collapsible view of the JSON plan. Keys that matter for tuning are highlighted. */
function JsonTree({ value, name, depth = 0 }: { value: unknown; name?: string; depth?: number }) {
  const [open, setOpen] = useState(depth < 3);
  const key = name !== undefined ? <span className="jk">{name}: </span> : null;
  if (value === null || typeof value !== "object") {
    const hot = name && /^(access_type|table_name|key|used_key_parts|rows_examined_per_scan|filtered|query_cost|read_cost|using_filesort|using_temporary_table)$/.test(name);
    return <div className={`jrow ${hot ? "hot" : ""}`}>{key}<span className={typeof value === "string" ? "js" : "jn"}>{JSON.stringify(value)}</span></div>;
  }
  const entries = Array.isArray(value) ? value.map((v, i) => [String(i), v] as const) : Object.entries(value as Record<string, unknown>);
  return (
    <div className="jnode">
      <div className="jrow toggle" onClick={() => setOpen(!open)}>{open ? "▾" : "▸"} {key}<span className="dim">{Array.isArray(value) ? `[${entries.length}]` : `{${entries.length}}`}</span></div>
      {open && <div className="jchildren">{entries.map(([k, v]) => <JsonTree key={k} name={k} value={v} depth={depth + 1} />)}</div>}
    </div>
  );
}

const fmtN = (n: number) => (n >= 100 ? Math.round(n).toLocaleString() : n >= 1 ? n.toFixed(1) : n.toFixed(3));
const planOf = (d: ExplainResult): PlanNode[] => { if (d.tree) return fromTimedTree(d.tree); try { return d.json ? fromJson(JSON.parse(d.json)) : []; } catch { return []; } };

/** The plan drawn as a tree: each step with its rows, its cost or measured time as a bar, and the slowest step marked. */
function PlanDiagram({ roots }: { roots: PlanNode[] }) {
  const [shut, setShut] = useState<Set<number>>(new Set());
  const all = flatten(roots);
  const measured = all.some((n) => n.self !== null);
  const value = (n: PlanNode) => (measured ? n.self : n.cost) ?? 0;
  const max = Math.max(...all.map(value), 0.0001);
  const hot = slowest(roots);
  const row = (n: PlanNode, depth: number): ReactElement[] => [
    <div key={n.id} className={`plan-row${hot?.id === n.id ? " hot" : ""}${n.warn ? " warn" : ""}`} style={{ paddingLeft: 8 + depth * 18 }}>
      <button className="plan-fold" disabled={n.children.length === 0} aria-label={shut.has(n.id) ? "Expand" : "Collapse"} onClick={() => setShut((s) => { const c = new Set(s); if (c.has(n.id)) c.delete(n.id); else c.add(n.id); return c; })}>{n.children.length ? (shut.has(n.id) ? "▸" : "▾") : "·"}</button>
      <div className="plan-main">
        <div className="plan-title"><b>{n.label}</b>{hot?.id === n.id && <span className="plan-badge">{measured ? "slowest step" : "most costly step"}</span>}{n.warn && <span className="plan-warn">{n.warn}</span>}</div>
        {n.detail && <div className="plan-detail dim small">{n.detail}</div>}
      </div>
      <div className="plan-num">{n.rows !== null && <span title="rows">{n.rows.toLocaleString()} rows</span>}{n.loops && n.loops > 1 ? <span title="times repeated">×{n.loops.toLocaleString()}</span> : null}</div>
      <div className="plan-bar" title={measured ? "time spent in this step alone" : "estimated cost"}><div style={{ width: `${Math.max(1, (value(n) / max) * 100)}%` }} /><span>{value(n) > 0 ? `${fmtN(value(n))}${measured ? " ms" : ""}` : ""}</span></div>
    </div>,
    ...(shut.has(n.id) ? [] : n.children.flatMap((c) => row(c, depth + 1))),
  ];
  return (
    <div className="plan-diagram" role="tree" aria-label="Query plan">
      <div className="plan-legend dim small">{measured ? "Bars show the time each step took on its own." : "Bars show the server's cost estimate. Use Analyze to measure real time."}</div>
      {roots.flatMap((r) => row(r, 0))}
    </div>
  );
}

/** "Faster than the run before": one line comparing this run with the previous run of the same statement. */
function Versus({ now, before }: { now: PlanNode[]; before: PlanNode[] }) {
  const a = summary(now), b = summary(before);
  if (!a || !b || a.unit !== b.unit || b.value === 0) return null;
  const change = ((a.value - b.value) / b.value) * 100;
  const word = Math.abs(change) < 1 ? "about the same as" : change < 0 ? `${Math.abs(change).toFixed(0)}% ${a.unit === "ms" ? "faster" : "cheaper"} than` : `${change.toFixed(0)}% ${a.unit === "ms" ? "slower" : "costlier"} than`;
  return <div className={`plan-versus ${change < -1 ? "good" : change > 1 ? "bad" : ""}`}><Icon name={change < -1 ? "check" : "warn"} size={13} /> {fmtN(a.value)} {a.unit === "ms" ? "ms" : "(cost)"}: {word} the previous run ({fmtN(b.value)} {a.unit === "ms" ? "ms" : "cost"}).</div>;
}

function ExplainView({ a }: { a: Extract<Analysis, { kind: "explain" | "analyze" }> }) {
  const d = a.data!;
  const roots = useMemo(() => planOf(d), [d]);
  const before = useMemo(() => (a.prev ? planOf(a.prev) : []), [a.prev]);
  const [tab, setTab] = useState<"diagram" | "plan" | "tree" | "json">(roots.length ? "diagram" : "plan");
  const doc = useMemo(() => { try { return d.json ? JSON.parse(d.json) : null; } catch { return null; } }, [d.json]);
  const warnings = d.hints.filter((h) => h.severity === "warning").length;
  return (
    <div className="analysis">
      <div className="hints">
        {d.hints.length === 0 && <div className="hint good"><Icon name="check" size={13} /> Nothing to worry about in this plan.</div>}
        {d.hints.map((h, i) => <div key={i} className={`hint ${h.severity}`}><span>{ICON[h.severity]}</span> {h.message}</div>)}
      </div>
      <div className="result-tabs sub">
        {roots.length > 0 && <button className={tab === "diagram" ? "active" : ""} onClick={() => setTab("diagram")}>Diagram</button>}
        <button className={tab === "plan" ? "active" : ""} onClick={() => setTab("plan")}>Plan{warnings ? ` (${warnings})` : ""}</button>
        {d.tree && <button className={tab === "tree" ? "active" : ""} onClick={() => setTab("tree")}>Timed tree</button>}
        {doc && <button className={tab === "json" ? "active" : ""} onClick={() => setTab("json")}>JSON</button>}
      </div>
      <div className="analysis-body">
        {tab === "diagram" && <>{before.length > 0 && <Versus now={roots} before={before} />}<PlanDiagram roots={roots} /></>}
        {tab === "plan" && <Grid set={d.plan} />}
        {tab === "tree" && d.tree && <pre className="tree-text">{d.tree}</pre>}
        {tab === "json" && doc && <JsonTree value={doc} />}
      </div>
    </div>
  );
}

function ProfileView({ a }: { a: Extract<Analysis, { kind: "profile" }> }) {
  const d = a.data!;
  const max = Math.max(...d.stages.map((s) => s.ms), 0.0001);
  const total = d.stages.reduce((n, s) => n + s.ms, 0);
  const fmt = (n: number) => (n >= 1 ? n.toFixed(2) : n.toFixed(3));
  return (
    <div className="analysis profile">
      {a.prev && a.prev.elapsed_ms > 0 && (() => {
        const ch = ((d.elapsed_ms - a.prev.elapsed_ms) / a.prev.elapsed_ms) * 100;
        return <div className={`plan-versus ${ch < -1 ? "good" : ch > 1 ? "bad" : ""}`}><Icon name={ch < -1 ? "check" : "warn"} size={13} /> {d.elapsed_ms} ms: {Math.abs(ch) < 1 ? "about the same as" : `${Math.abs(ch).toFixed(0)}% ${ch < 0 ? "faster" : "slower"} than`} the previous run ({a.prev.elapsed_ms} ms).</div>;
      })()}
      <div className="summary">
        <span><b>{d.elapsed_ms}</b> ms</span>
        <span><b>{d.rows_returned.toLocaleString()}</b> row(s) returned</span>
        {d.stages.length > 0 && <span><b>{fmt(total)}</b> ms in server stages</span>}
      </div>
      <div className="profile-cols">
        <section>
          <h4>Stages</h4>
          {d.stages.length === 0 ? <div className="dim small">{d.stages_note ?? "No stage timings available."}</div> : (
            <table className="stages">
              <tbody>
                {d.stages.map((s, i) => (
                  <tr key={i}>
                    <td className="sname">{s.name}</td>
                    <td className="sbar"><div style={{ width: `${Math.max(1, (s.ms / max) * 100)}%` }} /></td>
                    <td className="sms">{fmt(s.ms)} ms</td>
                    <td className="spct dim">{total > 0 ? `${((s.ms / total) * 100).toFixed(0)}%` : ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
        <section>
          <h4>Work done by the statement</h4>
          {d.counters.length === 0 ? <div className="dim small">No rows were read from tables.</div> : (
            <table className="counters">
              <tbody>
                {d.counters.map((c) => (
                  <tr key={c.name} className={/tmp_disk|full_join|Select_scan|merge_passes/.test(c.name) ? "warn" : ""}>
                    <td>{c.name}</td><td className="num">{c.value.toLocaleString()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <p className="dim small">Counters are the difference in this connection's session status around the statement.</p>
        </section>
      </div>
    </div>
  );
}

export default function AnalysisPane({ a }: { a: Analysis }) {
  const title = a.kind === "profile" ? "Profile" : a.kind === "analyze" ? "Explain Analyze" : "Explain";
  return (
    <div className="analysis-wrap">
      <div className="analysis-head"><b>{title}</b> <code className="dim">{a.sql.length > 140 ? a.sql.slice(0, 140) + "…" : a.sql}</code></div>
      {a.running && <div className="empty">{a.kind === "profile" ? "Profiling…" : a.kind === "analyze" ? "Running the query to time it…" : "Explaining…"}</div>}
      {a.error && <div className="error">{a.error}</div>}
      {!a.running && a.data && (a.kind === "profile" ? <ProfileView a={a as Extract<Analysis, { kind: "profile" }>} /> : <ExplainView a={a as Extract<Analysis, { kind: "explain" | "analyze" }>} />)}
    </div>
  );
}
