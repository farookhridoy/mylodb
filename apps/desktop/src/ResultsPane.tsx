import { useEffect, useState } from "react";
import AnalysisPane, { type Analysis } from "./AnalysisPane";
import type { StatementResult } from "./api";

export interface RunState {
  running: boolean;
  results: StatementResult[] | null;
  error: string | null;
  totalMs: number;
}

export default function ResultsPane({ run, analysis }: { run: RunState | undefined; analysis?: Analysis }) {
  const sets = (run?.results ?? []).flatMap((s) => s.sets.map((set) => ({ sql: s.sql, set })));
  const [tab, setTab] = useState<number | "messages" | "analysis">("messages");
  // a new analysis takes focus
  useEffect(() => { if (analysis) setTab("analysis"); }, [analysis]);

  // After each run, show the first result set when there is one, else the messages.
  useEffect(() => {
    if (!run?.running) setTab(sets.length > 0 ? 0 : "messages");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run?.results, run?.running]);

  if (!run && !analysis) return <div className="empty">Run a query with ⌘↵</div>;

  const selected = typeof tab === "number" ? sets[tab] : undefined;
  if (!run) return <div className="results"><div className="result-tabs"><button className="active">Analysis</button></div><div className="result-body">{analysis && <AnalysisPane a={analysis} />}</div></div>;
  return (
    <div className="results">
      <div className="result-tabs">
        {sets.map((_, i) => (
          <button key={i} className={tab === i ? "active" : ""} onClick={() => setTab(i)}>Result {i + 1}</button>
        ))}
        {analysis && <button className={tab === "analysis" ? "active" : ""} onClick={() => setTab("analysis")}>{analysis.kind === "profile" ? "Profile" : "Explain"}</button>}
        <button className={tab === "messages" ? "active" : ""} onClick={() => setTab("messages")}>
          Messages{run.results?.some((r) => r.error) ? " ⚠" : ""}
        </button>
        <span className="grow" />
        {run.running ? <span className="dim">Running…</span> : <span className="dim">{run.totalMs} ms</span>}
      </div>
      <div className="result-body">
        {run.error && <div className="error">{run.error}</div>}
        {tab === "analysis" && analysis ? <AnalysisPane a={analysis} /> : selected ? <Grid set={selected.set} /> : <Messages results={run.results ?? []} />}
      </div>
    </div>
  );
}

export function Grid({ set }: { set: StatementResult["sets"][number] }) {
  return (
    <>
      <div className="grid-scroll">
        <table className="grid">
          <thead>
            <tr><th className="rownum" />{set.columns.map((c, i) => <th key={i} title={c.type_name}>{c.name}</th>)}</tr>
          </thead>
          <tbody>
            {set.rows.map((r, i) => (
              <tr key={i}>
                <td className="rownum">{i + 1}</td>
                {r.map((v, j) => (v === null ? <td key={j} className="null">NULL</td> : <td key={j}>{v}</td>))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="grid-foot">
        {set.total_rows.toLocaleString()} row{set.total_rows === 1 ? "" : "s"}
        {set.truncated && ` · showing first ${set.rows.length.toLocaleString()}`}
      </div>
    </>
  );
}

function Messages({ results }: { results: StatementResult[] }) {
  if (results.length === 0) return <div className="empty">No statements</div>;
  return (
    <ul className="messages">
      {results.map((r, i) => (
        <li key={i} className={r.error ? "bad" : ""}>
          <code>{r.sql.length > 160 ? r.sql.slice(0, 160) + "…" : r.sql}</code>
          <div>
            {r.error
              ? r.error
              : r.sets.length > 0
                ? `${r.sets.map((s) => s.total_rows).join(" + ")} row(s) returned`
                : `OK, ${r.affected_rows} row(s) affected${r.last_insert_id ? `, last insert id ${r.last_insert_id}` : ""}`}
            {r.warnings > 0 && ` · ${r.warnings} warning(s)`}
            <span className="dim"> · {r.elapsed_ms} ms</span>
          </div>
        </li>
      ))}
    </ul>
  );
}
