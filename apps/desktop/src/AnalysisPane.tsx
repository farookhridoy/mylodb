import { useMemo, useState } from "react";
import type { ExplainResult, ProfileResult } from "./api";
import { Grid } from "./ResultsPane";

export type Analysis =
  | { kind: "explain" | "analyze"; sql: string; running: boolean; error: string | null; data: ExplainResult | null }
  | { kind: "profile"; sql: string; running: boolean; error: string | null; data: ProfileResult | null };

const ICON = { warning: "⚠", info: "ℹ", good: "✔" } as const;

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

function ExplainView({ a }: { a: Extract<Analysis, { kind: "explain" | "analyze" }> }) {
  const d = a.data!;
  const [tab, setTab] = useState<"plan" | "tree" | "json">(d.tree ? "tree" : "plan");
  const doc = useMemo(() => { try { return d.json ? JSON.parse(d.json) : null; } catch { return null; } }, [d.json]);
  const warnings = d.hints.filter((h) => h.severity === "warning").length;
  return (
    <div className="analysis">
      <div className="hints">
        {d.hints.length === 0 && <div className="hint good">✔ Nothing to worry about in this plan.</div>}
        {d.hints.map((h, i) => <div key={i} className={`hint ${h.severity}`}><span>{ICON[h.severity]}</span> {h.message}</div>)}
      </div>
      <div className="result-tabs sub">
        <button className={tab === "plan" ? "active" : ""} onClick={() => setTab("plan")}>Plan{warnings ? ` (${warnings} ⚠)` : ""}</button>
        {d.tree && <button className={tab === "tree" ? "active" : ""} onClick={() => setTab("tree")}>Timed tree</button>}
        {doc && <button className={tab === "json" ? "active" : ""} onClick={() => setTab("json")}>JSON</button>}
      </div>
      <div className="analysis-body">
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
