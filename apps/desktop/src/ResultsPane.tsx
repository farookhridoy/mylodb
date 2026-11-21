import { useEffect, useRef, useState } from "react";
import Icon, { type IconName } from "./Icon";
import InfoPane, { type InfoContext } from "./InfoPane";
import AnalysisPane, { type Analysis } from "./AnalysisPane";
import type { StatementResult } from "./api";

export interface RunState {
  running: boolean;
  results: StatementResult[] | null;
  error: string | null;
  totalMs: number;
}

type TabId = string; // "r0", "r1", … | "profile" | "explain" | "messages" | "data" | "info" | "history"

/**
 * The tabs under the console: Results, Profile, Messages, Table Data (table tabs), Info and History, in that order.
 * An Explain tab appears when a plan was requested.
 */
export default function ResultsPane({ run, analysis, data, dataLabel = "Table Data", onProfile, info, history }: {
  run: RunState | undefined; analysis?: Analysis;
  /** The table's own data grid (table tabs). It is the default view; running a query shows its result next to it. */
  data?: React.ReactNode; dataLabel?: string;
  /** Profile the statement at the cursor (the Profile tab offers it when there is no profile yet). */
  onProfile?(): void;
  info?: InfoContext;
  /** The history list, shown by the History tab. */
  history?: React.ReactNode;
}) {
  const sets = (run?.results ?? []).flatMap((s) => s.sets.map((set) => ({ sql: s.sql, set })));
  const hasData = data !== undefined;
  const [tab, setTab] = useState<TabId>(hasData ? "data" : "r0");

  // The grid can appear after the pane was first shown (the table tab was on Properties or Diagram meanwhile): show it then,
  // unless a query result is what the user was looking at.
  const hadData = useRef(hasData);
  useEffect(() => {
    if (hasData && !hadData.current) setTab((cur) => (cur === "r0" && !run ? "data" : cur));
    hadData.current = hasData;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasData]);
  // a new analysis takes focus
  useEffect(() => { if (analysis) setTab(analysis.kind === "profile" ? "profile" : "explain"); }, [analysis]);
  // After each run, show the first result set when there is one, else the messages.
  useEffect(() => {
    if (run && !run.running) setTab(sets.length > 0 ? "r0" : "messages");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run?.results, run?.running]);

  const explain = analysis && analysis.kind !== "profile" ? analysis : undefined;
  const profile = analysis && analysis.kind === "profile" ? analysis : undefined;
  const resultTabs: { id: TabId; label: string }[] = sets.length > 1 ? sets.map((_, i) => ({ id: `r${i}`, label: `Result ${i + 1}` })) : [{ id: "r0", label: "Results" }];
  const tabs: { id: TabId; label: string; icon: IconName; warn?: boolean }[] = [
    ...resultTabs.map((t) => ({ ...t, icon: "grid" as IconName })),
    ...(explain ? [{ id: "explain", label: "Explain", icon: "diagram" as IconName }] : []),
    { id: "profile", label: "Profile", icon: "pulse" },
    { id: "messages", label: "Messages", icon: "index", warn: !!run?.results?.some((r) => r.error) },
    ...(hasData ? [{ id: "data", label: dataLabel, icon: "table" as IconName }] : []),
    { id: "info", label: "Info", icon: "info" },
    { id: "history", label: "History", icon: "clock" },
  ];
  const showing = tabs.some((t) => t.id === tab) ? tab : tabs[0].id;
  const selected = showing.startsWith("r") && /^r\d+$/.test(showing) ? sets[Number(showing.slice(1))] : undefined;

  const body = () => {
    if (showing === "data") return data;
    if (showing === "explain") return explain ? <AnalysisPane a={explain} /> : null;
    if (showing === "profile") {
      return profile ? <AnalysisPane a={profile} /> : (
        <div className="empty">
          <p>Profile runs the statement at the cursor and shows where the time went.</p>
          {onProfile && <button onClick={onProfile}><Icon name="pulse" /> Profile the statement</button>}
        </div>
      );
    }
    if (showing === "messages") return run ? <Messages results={run.results ?? []} /> : <div className="empty">No messages yet. Run a query with ⌘↵.</div>;
    if (showing === "info") return info ? <InfoPane run={run} ctx={info} /> : null;
    if (showing === "history") return history ?? <div className="empty">No history</div>;
    if (selected) return <Grid set={selected.set} />;
    return <div className="empty">{run?.running ? "Running…" : "Run a query with ⌘↵"}</div>;
  };

  return (
    <div className="results">
      <div className="result-tabs" role="tablist">
        {tabs.map((t) => (
          <button key={t.id} role="tab" aria-selected={showing === t.id} className={showing === t.id ? "active" : ""} onClick={() => setTab(t.id)}>
            <Icon name={t.icon} size={12} /> {t.label}{t.warn ? <Icon name="warn" size={12} /> : null}
          </button>
        ))}
        <span className="grow" />
        {run ? (run.running ? <span className="dim">Running…</span> : <span className="dim">{run.totalMs} ms</span>) : null}
      </div>
      <div className={`result-body ${showing === "data" || showing === "history" ? "data" : ""}`}>
        {run?.error && showing !== "data" && showing !== "history" && showing !== "info" && <div className="error">{run.error}</div>}
        {body()}
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
